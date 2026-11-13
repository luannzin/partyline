//! Minigames. Each module owns its rules (host side), its bot, and its rendering (client side).
//! The host runs `tick`/`input`; every client renders the state it receives.
pub mod blink;
pub mod derby;
pub mod gridlock;
pub mod odd;

use std::time::Instant;

use ratatui::{buffer::Buffer, layout::Rect};
use serde::{Deserialize, Serialize};

use crate::gfx::{Canvas, Rgb, hue, pal};
use crate::proto::{Event, Input, Kind, Pid, Player};

/// Points for 1st, 2nd, 3rd; everyone else scores 0.
pub const POINTS: [u32; 3] = [4, 2, 1];

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Game {
    Blink(blink::S),
    Grid(gridlock::S),
    Derby(derby::S),
    Odd(odd::S),
}

impl Game {
    pub fn new(kind: Kind, players: &[Pid], seed: u64, now: u64) -> Game {
        match kind {
            Kind::Blink => Game::Blink(blink::S::new(players, seed, now)),
            Kind::Gridlock => Game::Grid(gridlock::S::new(players, seed, now)),
            Kind::Derby => Game::Derby(derby::S::new(players, seed, now)),
            Kind::Odd => Game::Odd(odd::S::new(players, seed, now)),
        }
    }
    pub fn kind(&self) -> Kind {
        match self {
            Game::Blink(_) => Kind::Blink,
            Game::Grid(_) => Kind::Gridlock,
            Game::Derby(_) => Kind::Derby,
            Game::Odd(_) => Kind::Odd,
        }
    }
    pub fn input(&mut self, p: Pid, i: &Input, now: u64, ev: &mut Vec<Event>) {
        match self {
            Game::Blink(s) => s.input(p, i, ev),
            Game::Grid(s) => s.input(p, i),
            Game::Derby(s) => s.input(p, i, now, ev),
            Game::Odd(s) => s.input(p, i, ev),
        }
    }
    /// Advance the simulation; returns the final ranking (best first) once the game ends.
    pub fn tick(&mut self, now: u64, ev: &mut Vec<Event>) -> Option<Vec<Pid>> {
        match self {
            Game::Blink(s) => s.tick(now, ev),
            Game::Grid(s) => s.tick(now, ev),
            Game::Derby(s) => s.tick(now),
            Game::Odd(s) => s.tick(now, ev),
        }
    }
    pub fn bot(&self, me: Pid, now: u64) -> Option<Input> {
        match self {
            Game::Blink(s) => s.bot(me, now),
            Game::Grid(s) => s.bot(me),
            Game::Derby(s) => s.bot(me, now),
            Game::Odd(s) => s.bot(me, now),
        }
    }
    /// Called on every received frame, before drawing: lets games timestamp what this client saw.
    pub fn observe(&self, l: &mut Local) {
        if let Game::Blink(s) = self {
            s.observe(l)
        }
    }
    pub fn on_event(&self, e: Event, v: &View, l: &mut Local, fx: &mut Fx) {
        match self {
            Game::Blink(s) => s.on_event(e, v, l, fx),
            Game::Grid(s) => s.on_event(e, v, l, fx),
            Game::Derby(s) => s.on_event(e, v, l, fx),
            Game::Odd(s) => s.on_event(e, v, l, fx),
        }
    }
    pub fn render(&self, v: &View, l: &mut Local, buf: &mut Buffer) {
        match self {
            Game::Blink(s) => s.render(v, l, buf),
            Game::Grid(s) => s.render(v, l, buf),
            Game::Derby(s) => s.render(v, l, buf),
            Game::Odd(s) => s.render(v, l, buf),
        }
    }
}

pub fn title(k: Kind) -> &'static str {
    match k {
        Kind::Blink => "BLINK",
        Kind::Gridlock => "GRIDLOCK",
        Kind::Derby => "KEYBOARD DERBY",
        Kind::Odd => "ODD ONE OUT",
    }
}

pub fn howto(k: Kind) -> (&'static str, &'static str) {
    match k {
        Kind::Blink => ("Wait for the flash, then hit it first. Too early and you're out.", "SPACE"),
        Kind::Gridlock => ("Steer your light trail. Touch any trail or wall and you crash.", "ARROWS / WASD"),
        Kind::Derby => ("Type each word to run. First across the line wins.", "TYPE"),
        Kind::Odd => ("Pick the answer the fewest people pick. The minority scores.", "1  2  3"),
    }
}

/// What a client is looking at this frame.
pub struct View<'a> {
    pub area: Rect,
    pub now: u64,
    pub t: f32,
    pub me: Pid,
    pub cam: Pid,
    pub players: &'a [Player],
    pub calm: bool,
}

impl View<'_> {
    pub fn name(&self, p: Pid) -> String {
        self.players.iter().find(|x| x.id == p).map(|x| x.name.to_uppercase()).unwrap_or_else(|| "GONE".into())
    }
    pub fn hue(&self, p: Pid) -> Rgb {
        self.players.iter().find(|x| x.id == p).map(|x| hue(x.hue)).unwrap_or(pal::DIM)
    }
}

/// Screen-wide juice requested by games and the app.
#[derive(Default)]
pub struct Fx {
    pub shake: f32,
    pub flash: f32,
    pub flash_c: Rgb,
}

impl Fx {
    pub fn kick(&mut self, shake: f32, flash: f32, c: Rgb) {
        self.shake = self.shake.max(shake);
        if flash > self.flash {
            self.flash = flash;
            self.flash_c = c;
        }
    }
}

#[derive(Clone, Copy)]
pub struct Part {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub life: f32,
    pub c: Rgb,
}

/// Per-client scratch state for the running game; reset when a new game starts.
#[derive(Default)]
pub struct Local {
    pub seen: Option<(u8, Instant)>,
    pub pressed: Option<u8>,
    pub typed: String,
    pub word: u16,
    pub typo_t: f32,
    pub race: Vec<(Pid, f32)>,
    pub parts: Vec<Part>,
    pub pt: f32,
    pub crashed: Vec<(Pid, f32)>,
    pub picked: Option<(u8, u8)>,
    pub reveal_t: f32,
}

impl Local {
    pub fn burst(&mut self, x: f32, y: f32, c: Rgb, n: usize, speed: f32, seed: u32) {
        for i in 0..n {
            let h = crate::gfx::hash(seed, i as u32);
            let a = (h % 6283) as f32 / 1000.0;
            let s = speed * (0.35 + (h >> 13) as f32 % 100.0 / 100.0);
            self.parts.push(Part { x, y, vx: a.cos() * s, vy: a.sin() * s * 0.6, life: 0.6 + (h % 50) as f32 / 100.0, c });
        }
    }
    /// Integrate and draw particles (canvas pixel space).
    pub fn draw_parts(&mut self, cv: &mut Canvas, dt: f32, gravity: f32) {
        self.parts.retain_mut(|p| {
            p.life -= dt;
            p.vy += gravity * dt;
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.life > 0.0
        });
        for p in &self.parts {
            cv.blend(p.x as i32, p.y as i32, p.c, (p.life * 2.0).min(1.0));
        }
    }
}

/// Backdrop inside the TV: the pre-rendered test chamber under its bare bulb, which buzzes and
/// now and then browns out. `energy` brightens it (0 = dim behind a busy game, 1 = the finale).
pub fn stage(area: Rect, t: f32, energy: f32) -> Canvas {
    let mut cv = Canvas::for_area(area, pal::BLACK);
    let brownout = if crate::gfx::hash((t * 7.0) as u32, 11) % 71 == 0 { 0.6 } else { 1.0 };
    let k = (0.5 + energy * 0.6) * brownout * (1.0 + 0.04 * (t * 13.0).sin() * (t * 4.1).sin());
    cv.cover(&crate::art::get().chamber, &|c| c.mul(k));
    cv
}

/// A flat panel (podium face / tile) with a hue stripe on top.
pub fn panel(cv: &mut Canvas, x: i32, y: i32, w: i32, h: i32, face: Rgb, stripe: Rgb) {
    cv.rect(x, y, w, h, face);
    cv.rect(x, y, w, 2, stripe);
    cv.rect(x, y + h - 1, w, 1, face.mul(0.6));
}
