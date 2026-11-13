//! Pre-rendered art baked by `tools/bake.py` (Blender renders and a distressed display font),
//! embedded in the binary and decoded once, on first use.
use std::sync::OnceLock;

use crate::gfx::Rgb;

pub struct Img {
    pub w: usize,
    pub h: usize,
    px: Vec<[u8; 4]>,
}

impl Img {
    fn decode(bytes: &[u8]) -> Img {
        let mut d = png::Decoder::new(std::io::Cursor::new(bytes));
        d.set_transformations(png::Transformations::EXPAND);
        let mut r = d.read_info().expect("baked asset is a valid png");
        let mut buf = vec![0; r.output_buffer_size().expect("asset fits in memory")];
        let info = r.next_frame(&mut buf).expect("baked asset is a valid png");
        let ch = info.color_type.samples();
        let px = buf[..info.buffer_size()]
            .chunks(ch)
            .map(|c| match ch {
                1 => [c[0], c[0], c[0], c[0]],
                3 => [c[0], c[1], c[2], 255],
                _ => [c[0], c[1], c[2], c[3]],
            })
            .collect();
        Img { w: info.width as usize, h: info.height as usize, px }
    }

    /// Average RGBA (0..1) over the source box [x0, x1) x [y0, y1), weighted by pixel overlap.
    /// Color is alpha-weighted so transparent edges don't darken sprites.
    pub fn area(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> [f32; 4] {
        let (cx0, cy0) = (x0.floor().max(0.0) as usize, y0.floor().max(0.0) as usize);
        let (cx1, cy1) = ((x1.ceil().max(0.0) as usize).min(self.w), (y1.ceil().max(0.0) as usize).min(self.h));
        let (mut rgb, mut a, mut wsum) = ([0.0f32; 3], 0.0f32, 0.0f32);
        for y in cy0..cy1 {
            let wy = (y1.min(y as f32 + 1.0) - y0.max(y as f32)).max(0.0);
            for x in cx0..cx1 {
                let w = wy * (x1.min(x as f32 + 1.0) - x0.max(x as f32)).max(0.0);
                let p = self.px[y * self.w + x];
                let wa = w * p[3] as f32;
                for i in 0..3 {
                    rgb[i] += p[i] as f32 * wa;
                }
                a += wa;
                wsum += w;
            }
        }
        if a <= 0.0 {
            return [0.0; 4];
        }
        [rgb[0] / a / 255.0, rgb[1] / a / 255.0, rgb[2] / a / 255.0, a / wsum / 255.0]
    }
}

pub fn rgb(p: [f32; 4]) -> Rgb {
    Rgb(p[0], p[1], p[2])
}

/// Display font atlas: one row of glyph cells for ASCII 32..126, cap height `cap` pixels.
pub struct Font {
    pub img: Img,
    pub cap: f32,
    pub pad: f32,
    x: Vec<f32>,
    adv: Vec<f32>,
}

impl Font {
    /// Cell start in the atlas and advance, both in atlas pixels.
    pub fn glyph(&self, c: char) -> (f32, f32) {
        let i = (c as usize).wrapping_sub(32);
        let i = if i < self.adv.len() { i } else { '?' as usize - 32 };
        (self.x[i], self.adv[i])
    }
}

pub struct Art {
    /// Dolly frames from the couch into the TV, stacked vertically (16:9 each).
    pub room_a: Img,
    /// Same frames lit by the TV only; alpha marks where the TV glass is visible.
    pub room_b: Img,
    pub frames: usize,
    pub frame_h: usize,
    /// Contestants 0..8 side by side (index = player hue), then the empty chair.
    pub crew: Img,
    pub chamber: Img,
    pub font: Font,
}

pub const EMPTY_CHAIR: usize = 8;

impl Art {
    /// Source rect (x, y, w, h) of contestant sprite `i`.
    pub fn crew_src(&self, i: usize) -> (f32, f32, f32, f32) {
        let w = (self.crew.w / 9) as f32;
        (w * i as f32, 0.0, w, self.crew.h as f32)
    }
}

pub fn get() -> &'static Art {
    static ART: OnceLock<Art> = OnceLock::new();
    ART.get_or_init(|| {
        let room_a = Img::decode(include_bytes!("../assets/room_a.png"));
        let frame_h = room_a.w * 9 / 16;
        let meta = include_str!("../assets/font.txt");
        let mut lines = meta.lines();
        let head: Vec<f32> = lines.next().unwrap_or("").split_whitespace().filter_map(|v| v.parse().ok()).collect();
        let adv: Vec<f32> = lines.next().unwrap_or("").split_whitespace().filter_map(|v| v.parse().ok()).collect();
        let (cap, pad) = (head[0], head[1]);
        let x = adv.iter().scan(0.0, |acc, a| {
            let at = *acc;
            *acc += a + 2.0 * pad;
            Some(at)
        });
        Art {
            frames: room_a.h / frame_h,
            frame_h,
            room_a,
            room_b: Img::decode(include_bytes!("../assets/room_b.png")),
            crew: Img::decode(include_bytes!("../assets/crew.png")),
            chamber: Img::decode(include_bytes!("../assets/chamber.png")),
            font: Font { img: Img::decode(include_bytes!("../assets/font.png")), cap, pad, x: x.collect(), adv },
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_decode_and_line_up() {
        let a = get();
        assert!(a.frames >= 2, "need at least two dolly frames");
        assert_eq!(a.room_a.h, a.frames * a.frame_h);
        assert_eq!((a.room_b.w, a.room_b.h), (a.room_a.w, a.room_a.h));
        assert_eq!(a.crew.w % 9, 0);
        assert_eq!(a.font.adv.len(), 95, "font covers ASCII 32..126");
        // The room shot shows some TV glass; the last frame is all glass.
        let glass = |y0: f32| a.room_b.area(0.0, y0, a.room_b.w as f32, y0 + a.frame_h as f32)[3];
        assert!(glass(0.0) > 0.05 && glass(0.0) < 0.5);
        assert!(glass(((a.frames - 1) * a.frame_h) as f32) > 0.95);
    }

    #[test]
    fn area_average_is_exact_on_a_flat_box() {
        let img = Img { w: 2, h: 1, px: vec![[0, 0, 0, 255], [255, 255, 255, 255]] };
        let v = img.area(0.5, 0.0, 1.5, 1.0);
        assert!((v[0] - 0.5).abs() < 1e-4);
    }
}
