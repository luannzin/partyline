//! Procedural sound effects: no asset files, every sound is a few synth segments.
use std::num::NonZero;
use std::time::Duration;

use rodio::Source;

#[derive(Clone, Copy)]
pub enum Sound {
    Tick,
    Select,
    Back,
    Static,
    Whoosh,
    Beep,
    Go,
    Flash,
    Crash,
    Typo,
    Word,
    Join,
    Reveal,
    Fanfare,
    Point,
}

#[derive(Clone, Copy)]
enum Wave {
    Square,
    Tri,
    Noise,
}

/// (wave, start Hz, end Hz, ms, volume)
type Seg = (Wave, f32, f32, u32, f32);

fn segs(s: Sound) -> Vec<Seg> {
    use Wave::*;
    match s {
        Sound::Tick => vec![(Square, 1250.0, 1250.0, 22, 0.05)],
        Sound::Select => vec![(Square, 620.0, 1240.0, 70, 0.07)],
        Sound::Back => vec![(Square, 760.0, 380.0, 70, 0.06)],
        Sound::Static => vec![(Noise, 0.0, 0.0, 130, 0.05)],
        Sound::Whoosh => vec![(Tri, 160.0, 900.0, 520, 0.12), (Noise, 0.0, 0.0, 120, 0.04)],
        Sound::Beep => vec![(Square, 880.0, 880.0, 110, 0.07)],
        Sound::Go => vec![(Square, 1320.0, 1320.0, 90, 0.08), (Square, 1760.0, 1760.0, 220, 0.08)],
        Sound::Flash => vec![(Square, 1600.0, 1400.0, 80, 0.09), (Noise, 0.0, 0.0, 90, 0.06)],
        Sound::Crash => vec![(Noise, 0.0, 0.0, 240, 0.12), (Square, 140.0, 40.0, 160, 0.08)],
        Sound::Typo => vec![(Square, 190.0, 160.0, 60, 0.06)],
        Sound::Word => vec![(Tri, 900.0, 1350.0, 45, 0.09)],
        Sound::Join => vec![(Tri, 660.0, 660.0, 70, 0.12), (Tri, 990.0, 990.0, 140, 0.12)],
        Sound::Reveal => vec![(Square, 300.0, 1200.0, 260, 0.06)],
        Sound::Point => vec![(Tri, 1180.0, 1180.0, 60, 0.1)],
        Sound::Fanfare => vec![
            (Square, 523.0, 523.0, 110, 0.07),
            (Square, 659.0, 659.0, 110, 0.07),
            (Square, 784.0, 784.0, 110, 0.07),
            (Square, 1047.0, 1047.0, 420, 0.07),
        ],
    }
}

const RATE: u32 = 44_100;

struct Synth {
    segs: Vec<Seg>,
    seg: usize,
    i: u32,
    phase: f32,
    seed: u32,
}

impl Iterator for Synth {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let &(wave, f0, f1, ms, vol) = self.segs.get(self.seg)?;
        let n = RATE * ms / 1000;
        if self.i >= n {
            self.seg += 1;
            self.i = 0;
            return self.next();
        }
        let t = self.i as f32 / n as f32;
        let f = f0 * (f1 / f0.max(1.0)).max(1e-3).powf(t);
        self.phase = (self.phase + f / RATE as f32).fract();
        self.seed = self.seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let raw = match wave {
            Wave::Square => if self.phase < 0.5 { 1.0 } else { -1.0 },
            Wave::Tri => 4.0 * (self.phase - 0.5).abs() - 1.0,
            Wave::Noise => (self.seed >> 16) as f32 / 32_768.0 - 1.0,
        };
        // 3 ms attack, exponential-ish decay: punchy, never clicks.
        let attack = (self.i as f32 / (RATE as f32 * 0.003)).min(1.0);
        let env = attack * (1.0 - t).powf(1.6);
        self.i += 1;
        Some(raw * env * vol)
    }
}

impl Source for Synth {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(1).unwrap()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(RATE).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_millis(self.segs.iter().map(|s| s.3 as u64).sum()))
    }
}

pub struct Sfx {
    sink: Option<rodio::MixerDeviceSink>,
}

impl Sfx {
    pub fn new(enabled: bool) -> Sfx {
        let sink = enabled.then(|| rodio::DeviceSinkBuilder::open_default_sink().ok()).flatten().map(|mut s| {
            s.log_on_drop(false);
            s
        });
        Sfx { sink }
    }
    pub fn play(&self, s: Sound) {
        if let Some(sink) = &self.sink {
            sink.mixer().add(Synth { segs: segs(s), seg: 0, i: 0, phase: 0.0, seed: 7 });
        }
    }
}
