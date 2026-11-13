//! Pixel canvas drawn with half-block cells, color math, easing curves, the display font,
//! image drawing, and buffer-level effects (shake, flash tint, CRT glass).
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

use crate::art::{self, Img};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rgb(pub f32, pub f32, pub f32);

impl Rgb {
    pub const fn hex(h: u32) -> Rgb {
        Rgb(
            ((h >> 16) & 255) as f32 / 255.0,
            ((h >> 8) & 255) as f32 / 255.0,
            (h & 255) as f32 / 255.0,
        )
    }
    pub fn lerp(self, o: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        Rgb(
            self.0 + (o.0 - self.0) * t,
            self.1 + (o.1 - self.1) * t,
            self.2 + (o.2 - self.2) * t,
        )
    }
    pub fn mul(self, k: f32) -> Rgb {
        Rgb(self.0 * k, self.1 * k, self.2 * k)
    }
    pub fn add(self, o: Rgb) -> Rgb {
        Rgb(self.0 + o.0, self.1 + o.1, self.2 + o.2)
    }
    pub fn grey(self) -> Rgb {
        let l = 0.3 * self.0 + 0.59 * self.1 + 0.11 * self.2;
        Rgb(l, l, l).lerp(pal::STAGE_DEEP, 0.35)
    }
    pub fn color(self) -> Color {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color::Rgb(c(self.0), c(self.1), c(self.2))
    }
    pub fn from_color(c: Color) -> Rgb {
        match c {
            Color::Rgb(r, g, b) => Rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0),
            _ => Rgb::default(),
        }
    }
}

/// Grimy basement palette: concrete, rust, bone-white paint, hazard amber, one bare bulb.
pub mod pal {
    use super::Rgb;
    pub const STAGE_DEEP: Rgb = Rgb::hex(0x121210);
    pub const PANEL: Rgb = Rgb::hex(0x1C1B16);
    pub const BULB: Rgb = Rgb::hex(0xFFC56A);
    pub const BULB_OFF: Rgb = Rgb::hex(0x3A3024);
    pub const PODIUM: Rgb = Rgb::hex(0x7A2A1E);
    pub const PODIUM_DARK: Rgb = Rgb::hex(0x3E1810);
    pub const WOOD: Rgb = Rgb::hex(0x5E4329);
    pub const INK: Rgb = Rgb::hex(0xE8E0C8);
    pub const DIM: Rgb = Rgb::hex(0x8F8A74);
    pub const GOLD: Rgb = Rgb::hex(0xE2A42C);
    pub const ORANGE: Rgb = Rgb::hex(0xC7562A);
    pub const RED: Rgb = Rgb::hex(0xD0352B);
    pub const GREEN: Rgb = Rgb::hex(0x8CC24A);
    pub const BLACK: Rgb = Rgb::hex(0x080807);
}

/// Player hues; contestant sprite `i` in the art wears hue `i` (same list in tools/scene.py).
const HUES: [u32; 8] = [
    0xD9533E, 0x4FA3C7, 0x8DB33A, 0xE39B2D, 0xC75B8E, 0x8C7BC9, 0x46B59A, 0xD8CB5A,
];

pub fn hue(i: u8) -> Rgb {
    Rgb::hex(HUES[i as usize % HUES.len()])
}

// ---------- easing ----------

/// CSS-style cubic-bezier easing, solved by bisection (robust for steep curves).
pub fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let b = |a: f32, c: f32, s: f32| {
        let u = 1.0 - s;
        3.0 * u * u * s * a + 3.0 * u * s * s * c + s * s * s
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) * 0.5;
        if b(x1, x2, mid) < t { lo = mid } else { hi = mid }
    }
    b(y1, y2, (lo + hi) * 0.5)
}
/// Strong ease-out: entering things, responses.
pub fn ease_out(t: f32) -> f32 {
    bezier(0.23, 1.0, 0.32, 1.0, t)
}
/// Strong ease-in-out: things moving across the screen (camera).
pub fn ease_in_out(t: f32) -> f32 {
    bezier(0.77, 0.0, 0.175, 1.0, t)
}
pub fn prog(now: f32, start: f32, dur: f32) -> f32 {
    ((now - start) / dur).clamp(0.0, 1.0)
}
pub fn hash(a: u32, b: u32) -> u32 {
    let mut h = a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 13)
}

// ---------- canvas ----------

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    px: Vec<Rgb>,
}

impl Canvas {
    /// Canvas covering `area` cells: one pixel per column, two per row.
    pub fn for_area(area: Rect, c: Rgb) -> Canvas {
        let (w, h) = (area.width as i32, area.height as i32 * 2);
        Canvas { w, h, px: vec![c; (w * h).max(0) as usize] }
    }
    pub fn get(&self, x: i32, y: i32) -> Rgb {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return pal::BLACK;
        }
        self.px[(y * self.w + x) as usize]
    }
    pub fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = c;
        }
    }
    pub fn blend(&mut self, x: i32, y: i32, c: Rgb, a: f32) {
        let o = self.get(x, y);
        self.set(x, y, o.lerp(c, a));
    }
    pub fn glow(&mut self, x: i32, y: i32, c: Rgb, a: f32) {
        let o = self.get(x, y);
        self.set(x, y, o.add(c.mul(a)));
    }
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, c);
            }
        }
    }
    /// Soft radial light, additive.
    pub fn light(&mut self, cx: f32, cy: f32, r: f32, c: Rgb, a: f32) {
        let (x0, x1) = ((cx - r) as i32, (cx + r) as i32);
        let (y0, y1) = ((cy - r) as i32, (cy + r) as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / r;
                if d < 1.0 {
                    self.glow(x, y, c, a * (1.0 - d).powi(2));
                }
            }
        }
    }
    /// Draw `src` (x, y, w, h in image pixels) of `img` into `dst` (canvas pixels), alpha-blended,
    /// each pixel passed through `tint`.
    pub fn img(&mut self, img: &Img, src: (f32, f32, f32, f32), dst: (f32, f32, f32, f32), tint: &dyn Fn(Rgb) -> Rgb) {
        let (kx, ky) = (src.2 / dst.2, src.3 / dst.3);
        for py in dst.1.floor() as i32..(dst.1 + dst.3).ceil() as i32 {
            for px in dst.0.floor() as i32..(dst.0 + dst.2).ceil() as i32 {
                if px < 0 || py < 0 || px >= self.w || py >= self.h {
                    continue;
                }
                let u0 = src.0 + (px as f32 - dst.0).max(0.0) * kx;
                let v0 = src.1 + (py as f32 - dst.1).max(0.0) * ky;
                let u1 = (src.0 + (px as f32 + 1.0 - dst.0) * kx).min(src.0 + src.2);
                let v1 = (src.1 + (py as f32 + 1.0 - dst.1) * ky).min(src.1 + src.3);
                let p = img.area(u0, v0, u1, v1);
                if p[3] > 0.01 {
                    self.blend(px, py, tint(art::rgb(p)), p[3]);
                }
            }
        }
    }

    /// Fill the whole canvas with `img` scaled to cover it (cropping the overflow), centered.
    pub fn cover(&mut self, img: &Img, tint: &dyn Fn(Rgb) -> Rgb) {
        let s = (self.w as f32 / img.w as f32).max(self.h as f32 / img.h as f32);
        let (sw, sh) = (self.w as f32 / s, self.h as f32 / s);
        let src = ((img.w as f32 - sw) / 2.0, (img.h as f32 - sh) / 2.0, sw, sh);
        self.img(img, src, (0.0, 0.0, self.w as f32, self.h as f32), tint);
    }

    /// Quantize to 6 bits per channel with a 4x4 ordered dither (a PS1-style grain, kept finer
    /// than the real 5 bits because terminal pixels are huge), then draw as half-block cells:
    /// one pixel per column, two per row.
    pub fn blit(&self, buf: &mut Buffer, area: Rect) {
        const BAYER: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
        let dither = |c: Rgb, x: i32, y: i32| {
            let b = BAYER[((y & 3) * 4 + (x & 3)) as usize] as f32 / 16.0;
            let q = |v: f32| (v.clamp(0.0, 1.0) * 63.0 + b).floor() / 63.0;
            Rgb(q(c.0), q(c.1), q(c.2)).color()
        };
        for cy in 0..area.height {
            for cx in 0..area.width {
                let (x, y) = (cx as i32, cy as i32 * 2);
                let cell = &mut buf[(area.x + cx, area.y + cy)];
                cell.set_symbol("▀");
                cell.fg = dither(self.get(x, y), x, y);
                cell.bg = dither(self.get(x, y + 1), x, y + 1);
            }
        }
    }
}

/// Looking at everything through the CRT glass: rounded tube corners, vignette, scanlines
/// (the lower pixel of every cell is dimmer), a slow rolling bar and a faint 60 Hz flicker.
pub fn crt_glass(buf: &mut Buffer, a: Rect, t: f32) {
    let (w, h) = (a.width as f32, a.height as f32);
    let band = (t * 0.11).fract() * 1.5 - 0.25;
    let flick = 1.0 + 0.015 * (t * 59.0).sin();
    for row in 0..a.height {
        let fy = (row as f32 + 0.5) / h;
        let ny = fy * 2.0 - 1.0;
        let roll = 1.0 + 0.06 * (1.0 - ((fy - band) / 0.07).abs()).max(0.0);
        for col in 0..a.width {
            let nx = (col as f32 + 0.5) / w * 2.0 - 1.0;
            let edge = (nx.abs().powi(20) + ny.abs().powi(20)).powf(1.0 / 20.0);
            let cell = &mut buf[(a.x + col, a.y + row)];
            if edge > 1.0 {
                cell.set_char(' ');
                cell.fg = pal::BLACK.color();
                cell.bg = pal::BLACK.color();
                continue;
            }
            let rim = 1.0 - 0.6 * ((edge - 0.95) / 0.05).clamp(0.0, 1.0);
            let k = (1.0 - 0.3 * (nx * nx * 0.6 + ny * ny)) * flick * roll * rim;
            cell.fg = Rgb::from_color(cell.fg).mul(k).color();
            cell.bg = Rgb::from_color(cell.bg).mul(k * 0.82).color();
        }
    }
}

/// Blend every cell toward `c` by `a` (flash, fades, CRT power).
pub fn tint(buf: &mut Buffer, area: Rect, c: Rgb, a: f32) {
    if a <= 0.001 {
        return;
    }
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buf[(x, y)];
            cell.fg = Rgb::from_color(cell.fg).lerp(c, a).color();
            cell.bg = Rgb::from_color(cell.bg).lerp(c, a).color();
        }
    }
}

/// Shift the whole frame by (dx, dy) cells: screen shake.
pub fn shift(buf: &mut Buffer, area: Rect, dx: i32, dy: i32) {
    if dx == 0 && dy == 0 {
        return;
    }
    let old = buf.clone();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let (sx, sy) = (x as i32 - dx, y as i32 - dy);
            let src = if sx >= area.left() as i32 && sy >= area.top() as i32 && sx < area.right() as i32 && sy < area.bottom() as i32 {
                old[(sx as u16, sy as u16)].clone()
            } else {
                let mut c = ratatui::buffer::Cell::default();
                c.set_symbol(" ");
                c.bg = pal::BLACK.color();
                c
            };
            buf[(x, y)] = src;
        }
    }
}

// ---------- type ----------
// Every word on screen is pixels, in one of two faces:
// - display: Bahnschrift Bold Condensed, distressed (baked by tools/bake.py), any size from
//   7 px caps up. Logo, titles, the TV menu's channel, names that matter, big numbers.
// - small: a crisp 5 px caps bitmap face with square DIN shapes to sit beside it, tabular
//   digits, 1 px drop shadow. Labels, hints, notes, values. 7 px line pitch.
// Text is set in caps; accented Latin letters fold to their base letter.

/// Cap height of the small face, and its line pitch.
pub const SMALL: i32 = 5;
pub const LINE: i32 = 7;

fn fold(c: char) -> char {
    match c.to_uppercase().next().unwrap_or(c) {
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'A',
        'Ç' => 'C',
        'È' | 'É' | 'Ê' | 'Ë' => 'E',
        'Ì' | 'Í' | 'Î' | 'Ï' => 'I',
        'Ñ' => 'N',
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => 'O',
        'Ù' | 'Ú' | 'Û' | 'Ü' => 'U',
        'Ý' => 'Y',
        u => u,
    }
}

/// Small-face glyph: width and five rows (bit `w - 1` is the left column).
fn glyph(c: char) -> (i32, [u8; 5]) {
    match fold(c) {
        'A' => (3, [0b111, 0b101, 0b111, 0b101, 0b101]),
        'B' => (3, [0b110, 0b101, 0b110, 0b101, 0b110]),
        'C' => (3, [0b111, 0b100, 0b100, 0b100, 0b111]),
        'D' => (3, [0b110, 0b101, 0b101, 0b101, 0b110]),
        'E' => (3, [0b111, 0b100, 0b110, 0b100, 0b111]),
        'F' => (3, [0b111, 0b100, 0b110, 0b100, 0b100]),
        'G' => (3, [0b111, 0b100, 0b101, 0b101, 0b111]),
        'H' => (3, [0b101, 0b101, 0b111, 0b101, 0b101]),
        'I' => (1, [1, 1, 1, 1, 1]),
        'J' => (3, [0b001, 0b001, 0b001, 0b101, 0b111]),
        'K' => (3, [0b101, 0b101, 0b110, 0b101, 0b101]),
        'L' => (3, [0b100, 0b100, 0b100, 0b100, 0b111]),
        'M' => (5, [0b10001, 0b11011, 0b10101, 0b10001, 0b10001]),
        'N' => (4, [0b1001, 0b1101, 0b1011, 0b1001, 0b1001]),
        'O' => (4, [0b1111, 0b1001, 0b1001, 0b1001, 0b1111]),
        'P' => (3, [0b111, 0b101, 0b111, 0b100, 0b100]),
        'Q' => (4, [0b1111, 0b1001, 0b1001, 0b1011, 0b1111]),
        'R' => (3, [0b110, 0b101, 0b110, 0b101, 0b101]),
        'S' => (3, [0b011, 0b100, 0b010, 0b001, 0b110]),
        'T' => (3, [0b111, 0b010, 0b010, 0b010, 0b010]),
        'U' => (3, [0b101, 0b101, 0b101, 0b101, 0b111]),
        'V' => (3, [0b101, 0b101, 0b101, 0b101, 0b010]),
        'W' => (5, [0b10001, 0b10001, 0b10101, 0b11011, 0b10001]),
        'X' => (3, [0b101, 0b101, 0b010, 0b101, 0b101]),
        'Y' => (3, [0b101, 0b101, 0b010, 0b010, 0b010]),
        'Z' => (3, [0b111, 0b001, 0b010, 0b100, 0b111]),
        '0' => (3, [0b111, 0b101, 0b101, 0b101, 0b111]),
        '1' => (3, [0b010, 0b110, 0b010, 0b010, 0b111]),
        '2' => (3, [0b111, 0b001, 0b111, 0b100, 0b111]),
        '3' => (3, [0b111, 0b001, 0b011, 0b001, 0b111]),
        '4' => (3, [0b101, 0b101, 0b111, 0b001, 0b001]),
        '5' => (3, [0b111, 0b100, 0b111, 0b001, 0b111]),
        '6' => (3, [0b111, 0b100, 0b111, 0b101, 0b111]),
        '7' => (3, [0b111, 0b001, 0b001, 0b001, 0b001]),
        '8' => (3, [0b111, 0b101, 0b111, 0b101, 0b111]),
        '9' => (3, [0b111, 0b101, 0b111, 0b001, 0b111]),
        ' ' => (2, [0; 5]),
        '.' => (1, [0, 0, 0, 0, 1]),
        ',' => (2, [0, 0, 0, 0b01, 0b10]),
        '\'' | '’' => (1, [1, 1, 0, 0, 0]),
        '"' => (3, [0b101, 0b101, 0, 0, 0]),
        '!' => (1, [1, 1, 1, 0, 1]),
        '?' => (3, [0b111, 0b001, 0b011, 0b000, 0b010]),
        '-' | '–' => (2, [0, 0, 0b11, 0, 0]),
        '+' => (3, [0, 0b010, 0b111, 0b010, 0]),
        '/' => (3, [0b001, 0b001, 0b010, 0b100, 0b100]),
        ':' => (1, [0, 1, 0, 1, 0]),
        '(' => (2, [0b01, 0b10, 0b10, 0b10, 0b01]),
        ')' => (2, [0b10, 0b01, 0b01, 0b01, 0b10]),
        '%' => (3, [0b101, 0b001, 0b010, 0b100, 0b101]),
        '=' => (3, [0, 0b111, 0, 0b111, 0]),
        '_' => (3, [0, 0, 0, 0, 0b111]),
        '*' => (3, [0, 0b101, 0b010, 0b101, 0]),
        '<' => (3, [0b001, 0b010, 0b100, 0b010, 0b001]),
        '>' => (3, [0b100, 0b010, 0b001, 0b010, 0b100]),
        '‹' => (2, [0, 0b01, 0b10, 0b01, 0]),
        '›' => (2, [0, 0b10, 0b01, 0b10, 0]),
        '·' => (1, [0, 0, 1, 0, 0]),
        '•' => (2, [0, 0, 0b11, 0b11, 0]),
        '●' => (5, [0b01110, 0b11111, 0b11111, 0b11111, 0b01110]),
        '★' => (5, [0b00100, 0b11111, 0b01110, 0b01010, 0b10001]),
        '✓' => (4, [0, 0b0001, 0b0010, 0b1010, 0b0100]),
        '✕' => (3, [0, 0b101, 0b010, 0b101, 0]),
        '…' => (5, [0, 0, 0, 0, 0b10101]),
        '↑' => (5, [0b00100, 0b01110, 0b10101, 0b00100, 0b00100]),
        '↓' => (5, [0b00100, 0b00100, 0b10101, 0b01110, 0b00100]),
        '←' => (5, [0b00100, 0b01000, 0b11111, 0b01000, 0b00100]),
        '→' => (5, [0b00100, 0b00010, 0b11111, 0b00010, 0b00100]),
        '|' | '▏' => (1, [1, 1, 1, 1, 1]),
        _ => (3, [0b111, 0b001, 0b011, 0b000, 0b010]),
    }
}

/// Width in pixels of small-face text with `track` pixels between letters.
pub fn label_width(s: &str, track: i32) -> i32 {
    let n = s.chars().count() as i32;
    s.chars().map(|c| glyph(c).0).sum::<i32>() + track * (n - 1).max(0)
}

/// `s`, cut with an ellipsis to fit `maxw` pixels.
pub fn fit_label(s: &str, maxw: i32, track: i32) -> String {
    if label_width(s, track) <= maxw {
        return s.to_string();
    }
    let mut t: String = s.to_string();
    while !t.is_empty() && label_width(&format!("{}…", t.trim_end()), track) > maxw {
        t.pop();
    }
    format!("{}…", t.trim_end())
}

/// The first wording of `says` (longest first) that fits `maxw` pixels; the last one, cut, when
/// none does.
pub fn pick_label(says: &[String], maxw: i32) -> String {
    says.iter().find(|s| label_width(s, 1) <= maxw).cloned().unwrap_or_else(|| fit_label(says.last().map_or("", |s| s), maxw, 1))
}

/// The end of `s` that fits `maxw` pixels (what is being typed stays in view).
pub fn tail_label(s: &str, maxw: i32) -> String {
    let mut t: Vec<char> = s.chars().collect();
    while !t.is_empty() && label_width(&t.iter().collect::<String>(), 1) > maxw {
        t.remove(0);
    }
    t.into_iter().collect()
}

/// Word-wrap to `maxw` pixels, balanced: the narrowest width that keeps the same number of
/// lines, so the last line is never a lonely word.
pub fn wrap_label(s: &str, maxw: i32) -> Vec<String> {
    let greedy = |w: i32| {
        let mut lines = vec![String::new()];
        for word in s.split_whitespace() {
            let cur = lines.last_mut().unwrap();
            let next = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if !cur.is_empty() && label_width(&next, 1) > w {
                lines.push(word.to_string());
            } else {
                *cur = next;
            }
        }
        lines
    };
    let mut best = greedy(maxw);
    let mut w = maxw;
    while w > 12 {
        w -= 2;
        let l = greedy(w);
        if l.len() > best.len() || l.iter().any(|x| label_width(x, 1) > maxw) {
            break;
        }
        best = l;
    }
    best
}

/// A key hint: the key on a bone keycap, then what it does. An empty key is a plain note.
pub type Hint<'a> = (&'a str, &'a str);

fn hint_width(h: &Hint) -> i32 {
    let label = label_width(h.1, 1);
    if h.0.is_empty() { label } else { label_width(h.0, 1) + 4 + 3 + label }
}

// ---------- display font ----------

fn font_k(cap: f32) -> f32 {
    cap / art::get().font.cap
}

/// Width in pixels of display text with caps `cap` pixels tall.
pub fn type_width(s: &str, cap: f32) -> i32 {
    let (f, k) = (&art::get().font, font_k(cap));
    (s.chars().map(|c| f.glyph(fold(c)).1).sum::<f32>() * k).ceil() as i32
}

/// Pixel rows display text with caps `cap` pixels tall covers, shadow included.
pub fn type_h(cap: f32) -> i32 {
    let f = &art::get().font;
    ((f.pad + f.cap) * font_k(cap)).ceil() as i32 + 1
}

pub fn big_width(s: &str, scale: i32) -> i32 {
    type_width(s, (7 * scale) as f32)
}

impl Canvas {
    /// Darken (or tint) rows `y0..y1` toward `c`, fading in from nothing at `y0` to `a` at `y1`:
    /// a floor shadow for text to sit on.
    pub fn band(&mut self, y0: i32, y1: i32, c: Rgb, a: f32) {
        for y in y0..y1 {
            let k = a * (y - y0 + 1) as f32 / (y1 - y0).max(1) as f32;
            for x in 0..self.w {
                self.blend(x, y, c, k);
            }
        }
    }

    /// Read a drawn frame back into pixels so more can be drawn over it (a half-block cell holds
    /// two pixels; any other cell reads as its background).
    pub fn from_buf(buf: &Buffer, area: Rect) -> Canvas {
        let mut cv = Canvas::for_area(area, pal::BLACK);
        for row in 0..area.height {
            for col in 0..area.width {
                let c = &buf[(area.x + col, area.y + row)];
                let bot = Rgb::from_color(c.bg);
                let top = if c.symbol() == "▀" { Rgb::from_color(c.fg) } else { bot };
                cv.set(col as i32, row as i32 * 2, top);
                cv.set(col as i32, row as i32 * 2 + 1, bot);
            }
        }
        cv
    }

    fn stamp(&mut self, x: i32, y: i32, s: &str, track: i32, c: Rgb, shadow: bool) -> i32 {
        for pass in [shadow, false] {
            let mut pen = x;
            for ch in s.chars() {
                let (w, rows) = glyph(ch);
                for (ry, bits) in rows.iter().enumerate() {
                    for rx in 0..w {
                        if bits >> (w - 1 - rx) & 1 == 1 {
                            let (px, py) = (pen + rx, y + ry as i32);
                            if pass { self.blend(px + 1, py + 1, pal::BLACK, 0.7) } else { self.set(px, py, c) }
                        }
                    }
                }
                pen += w + track;
            }
            if !shadow {
                break;
            }
        }
        label_width(s, track)
    }

    /// Small-face text with its drop shadow; `track` is the space between letters (1 for text,
    /// 2 for spaced headings). Returns the width.
    pub fn label(&mut self, x: i32, y: i32, s: &str, c: Rgb, track: i32) -> i32 {
        self.stamp(x, y, s, track, c, true)
    }

    pub fn label_center(&mut self, cx: i32, y: i32, s: &str, c: Rgb, track: i32) {
        self.label(cx - label_width(s, track) / 2, y, s, c, track);
    }

    /// A small heading centered on `cx`: spaced caps when they fit `maxw`, tighter (then cut)
    /// when they don't.
    pub fn heading(&mut self, cx: i32, y: i32, s: &str, c: Rgb, maxw: i32) {
        if label_width(s, 2) <= maxw {
            self.label_center(cx, y, s, c, 2);
        } else {
            self.label_center(cx, y, &fit_label(s, maxw, 1), c, 1);
        }
    }

    /// Small-face text on a plate, 7 px tall. Returns the plate's width.
    pub fn chip(&mut self, x: i32, y: i32, s: &str, fg: Rgb, bg: Rgb) -> i32 {
        let w = label_width(s, 1) + 4;
        self.rect(x, y, w, SMALL + 2, bg);
        self.stamp(x + 2, y + 1, s, 1, fg, false);
        w
    }

    pub fn chip_center(&mut self, cx: i32, y: i32, s: &str, fg: Rgb, bg: Rgb) {
        self.chip(cx - (label_width(s, 1) + 4) / 2, y, s, fg, bg);
    }

    /// Key hints centered on `cx`, 7 px tall: each key on a bone keycap, then a dim verb. When
    /// they don't all fit `maxw` they take turns, a page every three seconds.
    pub fn keys(&mut self, cx: i32, y: i32, maxw: i32, hints: &[Hint], t: f32) {
        const GAP: i32 = 8;
        let mut pages: Vec<(usize, usize, i32)> = vec![];
        let mut i = 0;
        while i < hints.len() {
            let (mut j, mut w) = (i, 0);
            while j < hints.len() {
                let hw = hint_width(&hints[j]) + if j > i { GAP } else { 0 };
                if j > i && w + hw > maxw {
                    break;
                }
                w += hw;
                j += 1;
            }
            pages.push((i, j, w));
            i = j;
        }
        let Some(&(a, b, w)) = pages.get((t / 3.0).max(0.0) as usize % pages.len().max(1)) else { return };
        let mut x = cx - w / 2;
        for (k, l) in &hints[a..b] {
            if !k.is_empty() {
                x += self.chip(x, y, k, pal::STAGE_DEEP, pal::INK.mul(0.78)) + 3;
            }
            x += self.label(x, y + 1, l, pal::DIM, 1) + GAP;
        }
    }

    /// Draw display text; `fill(px, py)` colors each pixel in text-local coordinates.
    pub fn big(&mut self, x: i32, y: i32, s: &str, scale: i32, fill: &dyn Fn(i32, i32) -> Rgb) {
        self.type_at(x, y, s, (7 * scale) as f32, fill);
    }

    /// Display text with caps `cap` pixels tall (any size, fractional too).
    pub fn type_at(&mut self, x: i32, y: i32, s: &str, cap: f32, fill: &dyn Fn(i32, i32) -> Rgb) {
        let (f, k) = (&art::get().font, font_k(cap));
        let rows = (f.img.h as f32 * k).ceil() as i32;
        let mut pen = 0.0f32;
        for ch in s.chars() {
            let (gx, adv) = f.glyph(fold(ch));
            let left = (x as f32 + pen - f.pad * k).floor() as i32;
            let cols = ((adv + 2.0 * f.pad) * k).ceil() as i32;
            for ty in 0..rows {
                for tx in 0..cols {
                    let (u, v) = (gx + tx as f32 / k, ty as f32 / k);
                    let cov = f.img.area(u, v, u + 1.0 / k, v + 1.0 / k)[3];
                    if cov > 0.04 {
                        let (px, py) = (left + tx, y + ty);
                        self.blend(px, py, fill(px - x, py - y), (cov * 1.3).min(1.0));
                    }
                }
            }
            pen += adv * k;
        }
    }

    /// Big text with a drop shadow, horizontally centered at `cx`.
    pub fn big_center(&mut self, cx: i32, y: i32, s: &str, scale: i32, fill: &dyn Fn(i32, i32) -> Rgb) {
        self.type_center(cx, y, s, (7 * scale) as f32, fill);
    }

    /// Display text with a drop shadow, horizontally centered at `cx`.
    pub fn type_center(&mut self, cx: i32, y: i32, s: &str, cap: f32, fill: &dyn Fn(i32, i32) -> Rgb) {
        let x = cx - type_width(s, cap) / 2;
        let d = ((cap / 21.0) as i32).max(1);
        self.type_at(x + d, y + d, s, cap, &|_, _| pal::BLACK.lerp(pal::STAGE_DEEP, 0.4));
        self.type_at(x, y, s, cap, fill);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_hits_endpoints_and_is_monotonic() {
        for f in [ease_out as fn(f32) -> f32, ease_in_out] {
            assert!(f(0.0).abs() < 1e-3 && (f(1.0) - 1.0).abs() < 1e-3);
            let mut last = -1.0;
            for i in 0..=100 {
                let v = f(i as f32 / 100.0);
                assert!(v >= last - 1e-4);
                last = v;
            }
        }
        assert!(ease_out(0.2) > 0.5, "ease-out must be front-loaded");
    }

    #[test]
    fn small_face_glyphs_fit_their_width_and_wrap_holds() {
        for c in (' '..='~').chain("·•●★✓✕…↑↓←→‹›ÃÇÉ".chars()) {
            let (w, rows) = glyph(c);
            assert!(rows.iter().all(|r| (*r as u32) < 1 << w), "glyph {c:?} spills past its width");
        }
        assert_eq!(label_width("AB", 1), 7);
        let lines = wrap_label("the quick brown fox jumps over the lazy dog", 40);
        assert!(lines.iter().all(|l| label_width(l, 1) <= 40));
        assert_eq!(lines.join(" "), "the quick brown fox jumps over the lazy dog");
        assert!(label_width(&fit_label("A VERY LONG ROOM NAME", 30, 1), 1) <= 30);
    }

    #[test]
    fn display_text_lands_inside_its_width() {
        let mut cv = Canvas { w: 120, h: 30, px: vec![pal::BLACK; 120 * 30] };
        let w = big_width("GO!", 2);
        assert!(w > 10 && w < 60);
        cv.big(4, 4, "GO!", 2, &|_, _| pal::INK);
        let lit: Vec<i32> = (0..120).filter(|&x| (0..30).any(|y| cv.get(x, y) != pal::BLACK)).collect();
        assert!(!lit.is_empty());
        assert!(*lit.first().unwrap() >= 0 && *lit.last().unwrap() <= 4 + w + 2);
    }
}
