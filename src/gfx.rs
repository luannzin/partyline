//! Pixel canvas drawn with half-block cells, color math, easing curves, a 5x7 pixel font,
//! and buffer-level effects (shake, flash tint).
use ratatui::{buffer::Buffer, layout::Rect, style::Color};

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

pub mod pal {
    use super::Rgb;
    // Living room
    pub const WALL_TOP: Rgb = Rgb::hex(0x14122C);
    pub const WALL_BOT: Rgb = Rgb::hex(0x1F1A3E);
    pub const FLOOR: Rgb = Rgb::hex(0x281912);
    pub const LAMP: Rgb = Rgb::hex(0xFFB24E);
    pub const TV_BODY: Rgb = Rgb::hex(0x2C2B34);
    pub const COUCH: Rgb = Rgb::hex(0x2E1A33);
    // Studio
    pub const STAGE: Rgb = Rgb::hex(0x2235B0);
    pub const STAGE_DEEP: Rgb = Rgb::hex(0x0A0F45);
    pub const BULB: Rgb = Rgb::hex(0xFFD25A);
    pub const BULB_OFF: Rgb = Rgb::hex(0x5A4A2A);
    pub const PODIUM: Rgb = Rgb::hex(0xC8243C);
    pub const PODIUM_DARK: Rgb = Rgb::hex(0x7E1428);
    pub const INK: Rgb = Rgb::hex(0xF6F1E4);
    pub const DIM: Rgb = Rgb::hex(0x98A2E6);
    pub const GOLD: Rgb = Rgb::hex(0xFFC23A);
    pub const ORANGE: Rgb = Rgb::hex(0xFF8A2A);
    pub const RED: Rgb = Rgb::hex(0xFF4058);
    pub const GREEN: Rgb = Rgb::hex(0x4BE38A);
    pub const BLACK: Rgb = Rgb::hex(0x05060F);
}

const HUES: [u32; 8] = [
    0xFF5A4E, 0x3EC6FF, 0x9BE34A, 0xFF9F1C, 0xFF4FB7, 0xA88BFF, 0x3EE6B0, 0xF5E663,
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
    pub fn fill(&mut self, f: impl Fn(i32, i32) -> Rgb) {
        for y in 0..self.h {
            for x in 0..self.w {
                self.px[(y * self.w + x) as usize] = f(x, y);
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
    pub fn blit(&self, buf: &mut Buffer, area: Rect) {
        for cy in 0..area.height {
            for cx in 0..area.width {
                let (x, y) = (cx as i32, cy as i32 * 2);
                let cell = &mut buf[(area.x + cx, area.y + cy)];
                cell.set_symbol("▀");
                cell.fg = self.get(x, y).color();
                cell.bg = self.get(x, y + 1).color();
            }
        }
    }
}

// ---------- text on cells ----------

/// Write text over whatever is drawn; keeps the pixel color behind as background.
pub fn text(buf: &mut Buffer, area: Rect, x: i32, y: i32, s: &str, fg: Rgb) {
    for (i, ch) in s.chars().enumerate() {
        let (cx, cy) = (x + i as i32, y);
        if cx < area.x as i32 || cy < area.y as i32 || cx >= area.right() as i32 || cy >= area.bottom() as i32 {
            continue;
        }
        let cell = &mut buf[(cx as u16, cy as u16)];
        if cell.symbol() == "▀" {
            cell.bg = Rgb::from_color(cell.fg).lerp(Rgb::from_color(cell.bg), 0.5).color();
        }
        cell.set_char(ch);
        cell.fg = fg.color();
    }
}

pub fn text_bg(buf: &mut Buffer, area: Rect, x: i32, y: i32, s: &str, fg: Rgb, bg: Rgb) {
    for (i, ch) in s.chars().enumerate() {
        let (cx, cy) = (x + i as i32, y);
        if cx < area.x as i32 || cy < area.y as i32 || cx >= area.right() as i32 || cy >= area.bottom() as i32 {
            continue;
        }
        let cell = &mut buf[(cx as u16, cy as u16)];
        cell.set_char(ch);
        cell.fg = fg.color();
        cell.bg = bg.color();
    }
}

pub fn ctext(buf: &mut Buffer, area: Rect, y: i32, s: &str, fg: Rgb) {
    let x = area.x as i32 + (area.width as i32 - s.chars().count() as i32) / 2;
    text(buf, area, x, y, s, fg);
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

// ---------- 5x7 pixel font ----------

fn glyph(c: char) -> &'static str {
    match c.to_ascii_uppercase() {
        'A' => ".###.|#...#|#...#|#####|#...#|#...#|#...#",
        'B' => "####.|#...#|#...#|####.|#...#|#...#|####.",
        'C' => ".###.|#...#|#....|#....|#....|#...#|.###.",
        'D' => "####.|#...#|#...#|#...#|#...#|#...#|####.",
        'E' => "#####|#....|#....|####.|#....|#....|#####",
        'F' => "#####|#....|#....|####.|#....|#....|#....",
        'G' => ".###.|#...#|#....|#.###|#...#|#...#|.####",
        'H' => "#...#|#...#|#...#|#####|#...#|#...#|#...#",
        'I' => ".###.|..#..|..#..|..#..|..#..|..#..|.###.",
        'J' => "..###|...#.|...#.|...#.|...#.|#..#.|.##..",
        'K' => "#...#|#..#.|#.#..|##...|#.#..|#..#.|#...#",
        'L' => "#....|#....|#....|#....|#....|#....|#####",
        'M' => "#...#|##.##|#.#.#|#.#.#|#...#|#...#|#...#",
        'N' => "#...#|#...#|##..#|#.#.#|#..##|#...#|#...#",
        'O' => ".###.|#...#|#...#|#...#|#...#|#...#|.###.",
        'P' => "####.|#...#|#...#|####.|#....|#....|#....",
        'Q' => ".###.|#...#|#...#|#...#|#.#.#|#..#.|.##.#",
        'R' => "####.|#...#|#...#|####.|#.#..|#..#.|#...#",
        'S' => ".####|#....|#....|.###.|....#|....#|####.",
        'T' => "#####|..#..|..#..|..#..|..#..|..#..|..#..",
        'U' => "#...#|#...#|#...#|#...#|#...#|#...#|.###.",
        'V' => "#...#|#...#|#...#|#...#|#...#|.#.#.|..#..",
        'W' => "#...#|#...#|#...#|#.#.#|#.#.#|#.#.#|.#.#.",
        'X' => "#...#|#...#|.#.#.|..#..|.#.#.|#...#|#...#",
        'Y' => "#...#|#...#|.#.#.|..#..|..#..|..#..|..#..",
        'Z' => "#####|....#|...#.|..#..|.#...|#....|#####",
        '0' => ".###.|#...#|#..##|#.#.#|##..#|#...#|.###.",
        '1' => "..#..|.##..|..#..|..#..|..#..|..#..|.###.",
        '2' => ".###.|#...#|....#|...#.|..#..|.#...|#####",
        '3' => "####.|....#|....#|.###.|....#|....#|####.",
        '4' => "...#.|..##.|.#.#.|#..#.|#####|...#.|...#.",
        '5' => "#####|#....|####.|....#|....#|#...#|.###.",
        '6' => ".###.|#....|#....|####.|#...#|#...#|.###.",
        '7' => "#####|....#|...#.|..#..|.#...|.#...|.#...",
        '8' => ".###.|#...#|#...#|.###.|#...#|#...#|.###.",
        '9' => ".###.|#...#|#...#|.####|....#|....#|.###.",
        '!' => "..#..|..#..|..#..|..#..|..#..|.....|..#..",
        '?' => ".###.|#...#|....#|...#.|..#..|.....|..#..",
        '.' => ".....|.....|.....|.....|.....|.....|..#..",
        ',' => ".....|.....|.....|.....|.....|..#..|.#...",
        '-' => ".....|.....|.....|.###.|.....|.....|.....",
        '+' => ".....|..#..|..#..|#####|..#..|..#..|.....",
        ':' => ".....|..#..|..#..|.....|..#..|..#..|.....",
        '\'' => "..#..|..#..|.#...|.....|.....|.....|.....",
        '/' => "....#|...#.|...#.|..#..|.#...|.#...|#....",
        '*' => ".....|#.#.#|.###.|#####|.###.|#.#.#|.....",
        ' ' => ".....|.....|.....|.....|.....|.....|.....",
        _ => ".###.|#...#|....#|...#.|..#..|.....|..#..",
    }
}

pub fn big_width(s: &str, scale: i32) -> i32 {
    let n = s.chars().count() as i32;
    if n == 0 { 0 } else { n * 6 * scale - scale }
}

impl Canvas {
    /// Draw pixel text; `fill(px, py)` colors each lit pixel in text-local coordinates.
    pub fn big(&mut self, x: i32, y: i32, s: &str, scale: i32, fill: &dyn Fn(i32, i32) -> Rgb) {
        for (i, ch) in s.chars().enumerate() {
            let ox = x + i as i32 * 6 * scale;
            for (row, line) in glyph(ch).split('|').enumerate() {
                for (col, b) in line.bytes().enumerate() {
                    if b != b'#' {
                        continue;
                    }
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let (lx, ly) = (i as i32 * 6 * scale + col as i32 * scale + sx, row as i32 * scale + sy);
                            self.set(ox + col as i32 * scale + sx, y + row as i32 * scale + sy, fill(lx, ly));
                        }
                    }
                }
            }
        }
    }

    /// Big text with a drop shadow, horizontally centered at `cx`.
    pub fn big_center(&mut self, cx: i32, y: i32, s: &str, scale: i32, fill: &dyn Fn(i32, i32) -> Rgb) {
        let x = cx - big_width(s, scale) / 2;
        let d = (scale / 3).max(1);
        self.big(x + d, y + d, s, scale, &|_, _| pal::BLACK.lerp(pal::STAGE_DEEP, 0.4));
        self.big(x, y, s, scale, fill);
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
    fn every_glyph_is_5x7() {
        for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!?.,-+:'/* ".chars() {
            let rows: Vec<_> = glyph(c).split('|').collect();
            assert_eq!(rows.len(), 7, "{c}");
            assert!(rows.iter().all(|r| r.len() == 5), "{c}");
        }
    }
}
