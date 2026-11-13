//! The living room. A night scene drawn analytically in world space (160 x 90 "pixels"),
//! seen through a camera so the menu can dolly straight into the TV screen.
use ratatui::layout::Rect;

use crate::gfx::{Canvas, Rgb, hash, pal};

pub const WW: f32 = 160.0;
pub const WH: f32 = 90.0;
/// TV screen in world space: x, y, w, h.
pub const SCREEN: (f32, f32, f32, f32) = (44.0, 11.0, 72.0, 42.0);

#[derive(Clone, Copy)]
pub struct Cam {
    pub cx: f32,
    pub cy: f32,
    pub zoom: f32,
}

impl Cam {
    pub fn room() -> Cam {
        Cam { cx: WW / 2.0, cy: WH / 2.0, zoom: 1.0 }
    }
    /// Camera that fills the terminal with the TV screen (cropping whichever side overflows).
    pub fn tv(area: Rect) -> Cam {
        let (w, h) = (area.width as f32, area.height as f32 * 2.0);
        let s0 = base_scale(area);
        let zoom = (w / (SCREEN.2 * s0)).max(h / (SCREEN.3 * s0)) * 1.02;
        Cam { cx: SCREEN.0 + SCREEN.2 / 2.0, cy: SCREEN.1 + SCREEN.3 / 2.0, zoom }
    }
    pub fn lerp(a: Cam, b: Cam, t: f32) -> Cam {
        Cam {
            cx: a.cx + (b.cx - a.cx) * t,
            cy: a.cy + (b.cy - a.cy) * t,
            zoom: (a.zoom.ln() + (b.zoom.ln() - a.zoom.ln()) * t).exp(),
        }
    }
}

fn base_scale(area: Rect) -> f32 {
    (area.width as f32 / WW).min(area.height as f32 * 2.0 / WH)
}

/// Where the TV screen lands in terminal cells for this camera.
pub fn screen_rect(area: Rect, cam: Cam) -> Rect {
    let s = base_scale(area) * cam.zoom;
    let (w, h) = (area.width as f32, area.height as f32 * 2.0);
    let x0 = ((SCREEN.0 - cam.cx) * s + w / 2.0).ceil().max(0.0);
    let x1 = ((SCREEN.0 + SCREEN.2 - cam.cx) * s + w / 2.0).floor().min(w);
    let y0 = (((SCREEN.1 - cam.cy) * s + h / 2.0) / 2.0).ceil().max(0.0);
    let y1 = (((SCREEN.1 + SCREEN.3 - cam.cy) * s + h / 2.0) / 2.0).floor().min(area.height as f32);
    Rect::new(area.x + x0 as u16, area.y + y0 as u16, (x1 - x0).max(0.0) as u16, (y1 - y0).max(0.0) as u16)
}

/// Paint the room onto a canvas (caller blits). `glow` is the color the TV throws into the room.
pub fn draw(area: Rect, cam: Cam, t: f32, glow: Rgb) -> Canvas {
    let mut cv = Canvas::for_area(area, pal::BLACK);
    let s = base_scale(area) * cam.zoom;
    let (w, h) = (cv.w as f32, cv.h as f32);
    cv.fill(|px, py| {
        let wx = (px as f32 + 0.5 - w / 2.0) / s + cam.cx;
        let wy = (py as f32 + 0.5 - h / 2.0) / s + cam.cy;
        let c = scene(wx, wy, t, glow);
        // Cinematic vignette in screen space.
        let (vx, vy) = (px as f32 / w - 0.5, py as f32 / h - 0.5);
        c.mul(1.0 - (vx * vx + vy * vy) * 0.9)
    });
    cv
}

fn rrect(x: f32, y: f32, x0: f32, y0: f32, w: f32, h: f32, r: f32) -> bool {
    let (cx, cy) = ((x - x0 - w / 2.0).abs() - (w / 2.0 - r), (y - y0 - h / 2.0).abs() - (h / 2.0 - r));
    let (ox, oy) = (cx.max(0.0), cy.max(0.0));
    (ox * ox + oy * oy).sqrt() + cx.max(cy).min(0.0) <= r
}

fn seg_dist(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let k = (((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((x - ax - dx * k).powi(2) + (y - ay - dy * k).powi(2)).sqrt()
}

/// Light thrown by the TV and the lamp onto room surfaces.
fn lit(c: Rgb, x: f32, y: f32, glow: Rgb, t: f32) -> Rgb {
    let (sx, sy) = (SCREEN.0 + SCREEN.2 / 2.0, SCREEN.1 + SCREEN.3 / 2.0);
    let d2 = (x - sx).powi(2) + ((y - sy) * 1.3).powi(2);
    let flicker = 1.0 + 0.04 * (t * 7.3).sin() * (t * 3.1).sin();
    let tv = glow.mul(0.85 * flicker / (1.0 + d2 / 900.0));
    let ld2 = (x - 156.0).powi(2) + (y - 22.0).powi(2);
    let lamp = pal::LAMP.mul(0.32 / (1.0 + ld2 / 260.0));
    Rgb(c.0 * (1.0 + tv.0 * 2.2) + tv.0 * 0.12 + lamp.0 * 0.5, c.1 * (1.0 + tv.1 * 2.2) + tv.1 * 0.12 + lamp.1 * 0.5, c.2 * (1.0 + tv.2 * 2.2) + tv.2 * 0.12 + lamp.2 * 0.5)
}

fn scene(x: f32, y: f32, t: f32, glow: Rgb) -> Rgb {
    // Couch and the two friends sitting on it (foreground, nearest first).
    let couch_top = 73.0 + ((x - 80.0) / 66.0).powi(2) * 2.0;
    let in_arm = rrect(x, y, 6.0, 76.0, 16.0, 20.0, 3.0) || rrect(x, y, 138.0, 76.0, 16.0, 20.0, 3.0);
    if in_arm || (x > 14.0 && x < 146.0 && y >= couch_top) {
        let mut c = pal::COUCH;
        if !in_arm && ((x - 58.0).abs() < 0.5 || (x - 102.0).abs() < 0.5) {
            c = c.mul(0.7);
        }
        let edge = if in_arm { (y - 76.0).abs() < 1.2 } else { y - couch_top < 1.3 };
        if edge {
            c = c.add(glow.mul(0.35));
        }
        return c.mul(0.9);
    }
    for (hx, hy, r) in [(62.0f32, 71.0f32, 6.5f32), (100.0, 72.0, 6.0)] {
        let d = ((x - hx).powi(2) + (y - hy).powi(2)).sqrt();
        if d <= r {
            let rim = d > r - 1.1 && y < hy - r * 0.2;
            return if rim { Rgb::hex(0x120E1A).add(glow.mul(0.55)) } else { Rgb::hex(0x0E0B15) };
        }
    }

    // The TV.
    let (sx, sy, sw, sh) = SCREEN;
    if rrect(x, y, sx, sy, sw, sh, 3.0) {
        let (nx, ny) = ((x - sx) / sw - 0.5, (y - sy) / sh - 0.5);
        let vig = 1.0 - (nx * nx + ny * ny) * 1.1;
        let sheen = if nx + ny < -0.55 && nx + ny > -0.62 { 0.06 } else { 0.0 };
        let base = pal::STAGE_DEEP.lerp(glow, 0.45);
        return base.mul(vig).add(Rgb(sheen, sheen, sheen));
    }
    if rrect(x, y, 36.0, 6.0, 88.0, 55.0, 5.0) {
        let mut c = pal::TV_BODY.mul(1.0 - (x - 36.0) / 88.0 * 0.25);
        if rrect(x, y, 41.0, 9.0, 78.0, 46.5, 4.0) {
            c = Rgb::hex(0x16151B);
        } else if y < 7.2 {
            c = c.mul(1.5);
        }
        if y > 55.0 && y < 59.5 && x > 48.0 && x < 104.0 && (x as i32 + y as i32) % 2 == 0 && y as i32 % 2 == 1 {
            c = Rgb::hex(0x1C1B22);
        }
        let led = ((x - 114.0).powi(2) + (y - 57.5).powi(2)).sqrt();
        if led < 1.1 {
            return Rgb::hex(0xFF3B4A);
        }
        if led < 2.6 {
            c = c.add(Rgb::hex(0xFF3B4A).mul(0.25 * (2.6 - led)));
        }
        return lit(c, x, y, glow, t).mul(0.9);
    }
    // Rabbit-ear antenna.
    for (bx, by) in [(66.0f32, 0.6f32), (95.0, 1.2)] {
        if seg_dist(x, y, 80.0, 6.0, bx, by) < 0.55 {
            return Rgb::hex(0x4A4A56);
        }
        if ((x - bx).powi(2) + (y - by).powi(2)).sqrt() < 1.1 {
            return Rgb::hex(0x7A7A88);
        }
    }

    // Console next to the TV, with a breathing LED.
    if rrect(x, y, 127.0, 56.5, 11.0, 4.5, 1.0) {
        let led = ((x - 136.0).powi(2) + (y - 58.6).powi(2)).sqrt() < 0.7;
        if led {
            return pal::GREEN.mul(0.5 + 0.5 * (t * 2.0).sin().abs());
        }
        let c = if (y - 58.6).abs() < 0.4 && x > 129.0 && x < 133.0 { Rgb::hex(0x2A2A33) } else { Rgb::hex(0xCFCBD8).mul(0.55) };
        return lit(c, x, y, glow, t);
    }
    // Cabinet.
    if x > 24.0 && x < 140.0 && y >= 61.0 && y < 71.0 {
        let mut c = Rgb::hex(0x3B2416);
        if y < 62.0 {
            c = Rgb::hex(0x5A3926);
        }
        if (x - 82.0).abs() < 0.4 || ((x - 78.5).powi(2) + (y - 66.0).powi(2)).sqrt() < 0.7 || ((x - 85.5).powi(2) + (y - 66.0).powi(2)).sqrt() < 0.7 {
            c = c.mul(0.6);
        }
        return lit(c, x, y, glow, t);
    }

    // Window with a night city.
    if x > 0.0 && x < 30.0 && y > 4.0 && y < 42.0 {
        if !(x > 4.0 && x < 26.0 && y > 7.0 && y < 39.0) || (x - 15.0).abs() < 0.5 || (y - 22.0).abs() < 0.5 {
            return lit(Rgb::hex(0x2D2850), x, y, glow, t);
        }
        let moon = ((x - 10.0).powi(2) + (y - 12.0).powi(2)).sqrt();
        if moon < 2.6 {
            return Rgb::hex(0xF2E7C2);
        }
        let mut c = Rgb::hex(0x070B26).lerp(Rgb::hex(0x1A2756), (y - 7.0) / 32.0).add(Rgb::hex(0x302A10).mul((4.0 - moon).max(0.0) / 4.0));
        let col = (x / 3.0) as u32;
        let top = 26.0 + (hash(col, 7) % 9) as f32;
        if y > top {
            c = Rgb::hex(0x0A0D24);
            let (wx, wy) = (x as u32, y as u32);
            if wx % 2 == 0 && wy % 2 == 0 && hash(wx, wy) % 5 == 0 {
                let on = hash(wx, wy + (t * 0.2) as u32) % 7 != 0;
                if on {
                    c = Rgb::hex(0xFFCF73).mul(0.8);
                }
            }
        } else {
            let (ix, iy) = (x as u32, y as u32);
            if hash(ix * 3, iy * 5) % 97 == 0 {
                c = c.add(Rgb(0.6, 0.6, 0.7).mul(0.5 + 0.5 * (t * 1.7 + ix as f32).sin()));
            }
        }
        return c;
    }
    // Curtains.
    if (x > -4.0 && x < 4.0 || x > 26.0 && x < 33.0) && y > 2.0 && y < 46.0 {
        return lit(Rgb::hex(0x47152E).mul(0.8 + 0.2 * (x * 2.4).sin()), x, y, glow, t);
    }
    // Poster: the show's own key art.
    if x > 136.0 && x < 151.0 && y > 8.0 && y < 31.0 {
        let frame = x < 137.0 || x > 150.0 || y < 9.0 || y > 30.0;
        let c = if frame {
            Rgb::hex(0x2B2748)
        } else if ((x - 143.5).abs() + (y - 18.5).abs()) < 5.0 {
            pal::GOLD
        } else if (x as i32 + y as i32) % 3 == 0 && (y < 11.0 || y > 27.0) {
            pal::BULB
        } else {
            pal::PODIUM
        };
        return lit(c.mul(0.75), x, y, glow, t);
    }
    // Floor lamp.
    let shade_w = 4.0 + (y - 15.0) * 0.35;
    if y > 15.0 && y < 25.0 && (x - 157.0).abs() < shade_w {
        return pal::LAMP.lerp(pal::INK, 0.15 * (1.0 - (y - 15.0) / 10.0));
    }
    if (x - 157.0).abs() < 0.6 && y >= 25.0 && y < 66.0 {
        return Rgb::hex(0x2A2A30);
    }

    // Wall, baseboard, floor and rug.
    let c = if y < 63.0 {
        let base = pal::WALL_TOP.lerp(pal::WALL_BOT, y / 63.0);
        if (x / 5.0) as i32 % 2 == 0 { base.mul(1.07) } else { base }
    } else if y < 65.0 {
        Rgb::hex(0x2A2442)
    } else if ((x - 80.0) / 72.0).powi(2) + ((y - 84.0) / 10.0).powi(2) < 1.0 {
        let ring = ((x - 80.0) / 72.0).powi(2) + ((y - 84.0) / 10.0).powi(2) > 0.8;
        if ring { Rgb::hex(0x5A2448) } else { Rgb::hex(0x3A1A36) }
    } else {
        let plank = (y - 65.0) / 3.0;
        let joint = ((x + (plank as i32 % 3) as f32 * 13.0) / 26.0).fract() < 0.03;
        if plank.fract() < 0.12 || joint { pal::FLOOR.mul(0.75) } else { pal::FLOOR }
    };
    let mut c = lit(c, x, y, glow, t);
    // Pool of TV light on the floor.
    if y > 65.0 {
        let k = ((x - 80.0) / 50.0).powi(2) + ((y - 68.0) / 6.0).powi(2);
        if k < 1.0 {
            c = c.add(glow.mul(0.12 * (1.0 - k)));
        }
    }
    c
}
