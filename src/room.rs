//! The living room, pre-rendered in Blender (`tools/scene.py`): a dolly from behind the couch
//! straight into the CRT. Two light passes (bare bulb, TV) are mixed live, so the TV glow follows
//! the highlighted menu item and the bulb buzzes; the game draws the TV glass where the mask says.
use ratatui::layout::Rect;

use crate::art;
use crate::gfx::{Canvas, Rgb, hash, pal};

pub struct Shot {
    pub cv: Canvas,
    /// TV glass bounds in canvas pixels: x0, y0, x1, y1 (inclusive). Empty when out of view.
    pub glass: (i32, i32, i32, i32),
}

/// Where the camera rests for the menu: the widest shot whose TV glass still has `w` x `h`
/// canvas pixels for the menu. Small windows move the camera in along the dolly.
pub fn rest(area: Rect, w: f32, h: f32) -> f32 {
    let art = art::get();
    let (cw, ch) = (area.width as f32, area.height as f32 * 2.0);
    let (iw, ih) = (art.room_a.w as f32, art.frame_h as f32);
    let s = (cw / iw).max(ch / ih);
    let (ox, oy) = ((iw - cw / s) / 2.0, (ih - ch / s) / 2.0);
    let last = art.frames - 1;
    (0..=50)
        .map(|i| i as f32 / 50.0)
        .find(|&p| {
            let f = p * last as f32;
            let (i0, m) = (f.floor() as usize, f.fract());
            let i1 = (i0 + 1).min(last);
            let g = |k: usize| art.glass[i0][k] + (art.glass[i1][k] - art.glass[i0][k]) * m;
            let gw = ((g(2) - ox) * s).min(cw) - ((g(0) - ox) * s).max(0.0);
            let gh = ((g(3) - oy) * s).min(ch) - ((g(1) - oy) * s).max(0.0);
            gw >= w && gh >= h
        })
        .unwrap_or(1.0)
}

/// `p`: 0 = the room shot from behind the couch, 1 = the glass fills the view.
pub fn draw(area: Rect, p: f32, t: f32, glow: Rgb) -> Shot {
    let art = art::get();
    let mut cv = Canvas::for_area(area, pal::BLACK);
    let f = p.clamp(0.0, 1.0) * (art.frames - 1) as f32;
    let i0 = f.floor() as usize;
    let (i1, mix) = ((i0 + 1).min(art.frames - 1), f.fract());
    let (iw, ih) = (art.room_a.w as f32, art.frame_h as f32);
    let s = (cv.w as f32 / iw).max(cv.h as f32 / ih);
    let (ox, oy) = ((iw - cv.w as f32 / s) / 2.0, (ih - cv.h as f32 / s) / 2.0);

    // Bare bulb: a steady buzz and the odd brownout. The TV breathes a little.
    let brownout = if hash((t * 9.0) as u32, 3) % 61 == 0 { 0.5 } else { 1.0 };
    let bulb = 1.4 * brownout * (1.0 + 0.05 * (t * 13.0).sin() * (t * 5.3).sin());
    let tv = 1.5 * (1.0 + 0.04 * (t * 21.0).sin());
    let glass_base = pal::BLACK.lerp(glow, 0.3 * (1.0 - p * 0.85));
    let frame = (t * 30.0) as u32;

    let sample = |img: &art::Img, u: f32, v: f32| {
        let at = |i: usize| {
            let y0 = (i * art.frame_h) as f32 + v;
            img.area(u, y0, u + 1.0 / s, (y0 + 1.0 / s).min(((i + 1) * art.frame_h) as f32))
        };
        let a = at(i0);
        if mix < 0.01 {
            return a;
        }
        let b = at(i1);
        [0, 1, 2, 3].map(|k| a[k] + (b[k] - a[k]) * mix)
    };

    let mut bb = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    let (w, h) = (cv.w as f32, cv.h as f32);
    for y in 0..cv.h {
        for x in 0..cv.w {
            let (u, v) = (ox + x as f32 / s, oy + y as f32 / s);
            let a = sample(&art.room_a, u, v);
            let b = sample(&art.room_b, u, v);
            let lit = Rgb(a[0], a[1], a[2]).mul(bulb).add(Rgb(b[0] * glow.0, b[1] * glow.1, b[2] * glow.2).mul(tv));
            let mut c = grade(lit);
            if b[3] > 0.01 {
                // TV glass: dim phosphor in the glow color, pixel scanlines, grain, and the room's
                // reflection from the bulb pass.
                let grain = (hash(x as u32 ^ frame.wrapping_mul(7919), y as u32) % 100) as f32 * 0.0004;
                let line = if y % 2 == 1 { 0.72 } else { 1.0 };
                let g = glass_base.mul(line).add(Rgb(a[0], a[1], a[2]).mul(0.3 * bulb)).add(Rgb(grain, grain, grain));
                c = c.lerp(g, b[3]);
                if b[3] > 0.5 {
                    bb = (bb.0.min(x), bb.1.min(y), bb.2.max(x), bb.3.max(y));
                }
            }
            let (vx, vy) = (x as f32 / w - 0.5, y as f32 / h - 0.5);
            cv.set(x, y, c.mul(1.0 - (vx * vx + vy * vy) * 0.8));
        }
    }
    Shot { cv, glass: bb }
}

/// Grimy film grade: gentle S-curve, warm highlights, shadows lifted toward olive.
fn grade(c: Rgb) -> Rgb {
    let s = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        v * 0.6 + v * v * (3.0 - 2.0 * v) * 0.4
    };
    Rgb(s(c.0), s(c.1) * 0.97, s(c.2) * 0.86).add(Rgb::hex(0x0C0D09))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_shot_frames_the_tv_and_dolly_fills_it() {
        let a = Rect::new(0, 0, 120, 36);
        let (x0, _, x1, _) = draw(a, 0.0, 1.0, pal::INK).glass;
        assert!(x1 - x0 > 120 / 4 && x1 - x0 < 120 * 2 / 3, "TV glass is a big part of the room shot");
        assert!(x0 > 0 && x1 < 119, "and fully inside it");
        assert_eq!(draw(a, 1.0, 1.0, pal::INK).glass, (0, 0, 119, 71), "the dolly ends inside the glass");
        let p = rest(Rect::new(0, 0, 80, 24), 60.0, 46.0);
        assert!(p > 0.0 && p < 1.0, "a small window moves the camera in, not all the way");
        assert_eq!(rest(Rect::new(0, 0, 240, 64), 60.0, 46.0), 0.0, "a big one keeps the wide shot");
    }
}
