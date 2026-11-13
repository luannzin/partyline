//! KEYBOARD DERBY: typing race. Clients check letters locally and only report finished words,
//! so typing never waits on the network.
use ratatui::buffer::Buffer;
use serde::{Deserialize, Serialize};

use super::{Fx, Local, View, stage};
use crate::gfx::{LINE, Rgb, SMALL, big_width, fit_label, hash, label_width, pal, type_h};
use crate::proto::{Event, Input, Pid};

pub const LEN: usize = 16;
const LIMIT: u64 = 60_000;

const WORDS: &[&str] = &[
    "party", "neon", "rocket", "pizza", "laser", "banana", "dragon", "turbo", "pixel", "jungle", "comet", "taco",
    "ninja", "piano", "zebra", "lemon", "storm", "candy", "robot", "mango", "tiger", "disco", "magic", "panda",
    "ocean", "ghost", "spicy", "glitch", "cactus", "waffle", "thunder", "galaxy", "bubble", "pepper", "monkey",
    "frozen", "rhythm", "jacket", "planet", "cookie", "puzzle", "violet", "summer", "hammer", "castle", "orbit",
    "wizard", "falcon", "noodle", "volcano", "cheese", "marble", "sprint", "anchor", "button", "copper", "sunset",
    "pickle", "radio", "velvet", "yellow", "quartz", "shadow", "lizard", "bonus", "combo", "arcade", "joystick",
    "buzzer", "trophy", "confetti", "spotlight", "champion", "victory", "boost", "speed", "flash", "crown",
];

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct S {
    pub words: Vec<String>,
    pub progress: Vec<(Pid, u16)>,
    pub finished: Vec<Pid>,
    start: u64,
    seed: u64,
}

pub enum Key {
    Ok,
    Word(Input),
    Typo,
    Ignore,
}

impl S {
    pub fn new(players: &[Pid], seed: u64, now: u64) -> S {
        let words = (0..LEN).map(|i| WORDS[hash(seed as u32, i as u32) as usize % WORDS.len()].to_string()).collect();
        S { words, progress: players.iter().map(|&p| (p, 0)).collect(), finished: vec![], start: now, seed }
    }

    fn of(&self, p: Pid) -> u16 {
        self.progress.iter().find(|e| e.0 == p).map(|e| e.1).unwrap_or(0)
    }

    pub fn input(&mut self, p: Pid, i: &Input, _now: u64, ev: &mut Vec<Event>) {
        let Input::Word(idx) = *i else { return };
        let Some(e) = self.progress.iter_mut().find(|e| e.0 == p) else { return };
        if e.1 != idx || e.1 as usize >= LEN {
            return;
        }
        e.1 += 1;
        ev.push(Event::Word(p));
        if e.1 as usize == LEN {
            self.finished.push(p);
            ev.push(Event::Finish(p));
        }
    }

    pub fn tick(&mut self, now: u64) -> Option<Vec<Pid>> {
        if self.finished.len() < self.progress.len() && now < self.start + LIMIT {
            return None;
        }
        let mut rest: Vec<(Pid, u16)> = self.progress.iter().filter(|e| !self.finished.contains(&e.0)).copied().collect();
        rest.sort_by_key(|e| std::cmp::Reverse(e.1));
        Some(self.finished.iter().copied().chain(rest.into_iter().map(|e| e.0)).collect())
    }

    pub fn bot(&self, me: Pid, now: u64) -> Option<Input> {
        let done = self.of(me);
        if done as usize >= LEN {
            return None;
        }
        let cps = 3.0 + (hash(self.seed as u32 ^ 0xD3, me as u32) % 32) as f32 / 10.0;
        let typed = now.saturating_sub(self.start + 700) as f32 / 1000.0 * cps;
        let mut need = 0.0;
        for (i, w) in self.words.iter().enumerate() {
            need += w.len() as f32 + 1.0;
            if need > typed {
                return (i as u16 > done).then_some(Input::Word(done));
            }
        }
        Some(Input::Word(done))
    }

    /// Local keystroke for player `me`.
    pub fn key(&self, l: &mut Local, me: Pid, c: char) -> Key {
        let idx = l.word.max(self.of(me)) as usize;
        let Some(w) = self.words.get(idx) else { return Key::Ignore };
        let next = format!("{}{}", l.typed, c.to_ascii_lowercase());
        if !w.starts_with(&next) {
            return Key::Typo;
        }
        if next == *w {
            l.typed.clear();
            l.word = idx as u16 + 1;
            return Key::Word(Input::Word(idx as u16));
        }
        l.typed = next;
        Key::Ok
    }

    pub fn on_event(&self, e: Event, v: &View, l: &mut Local, fx: &mut Fx) {
        if let Event::Finish(p) = e {
            if p == v.me && !v.calm {
                fx.kick(0.8, 0.3, pal::GOLD);
            }
            l.reveal_t = v.t;
        }
    }

    /// Seconds left on the clock.
    pub fn left(&self, now: u64) -> u64 {
        (self.start + LIMIT).saturating_sub(now) / 1000
    }

    pub fn render(&self, v: &View, l: &mut Local, buf: &mut Buffer) {
        let a = v.area;
        let mut cv = stage(a, v.t, 0.2);
        let dt = (v.t - l.pt).clamp(0.0, 0.1);
        l.pt = v.t;

        // Smooth racer positions toward true progress.
        for &(p, n) in &self.progress {
            let target = n as f32 / LEN as f32;
            match l.race.iter_mut().find(|e| e.0 == p) {
                Some(e) => e.1 += (target - e.1) * (1.0 - (-dt * 10.0).exp()),
                None => l.race.push((p, target)),
            }
        }

        // Lanes with each racer's name in its color at the start line.
        let lane_h = ((cv.h - 2 - 4 - type_h(7.0) - 2 - LINE) / (self.progress.len() as i32).max(1)).clamp(6, 7);
        let top = 2;
        let names: Vec<String> = self.progress.iter().map(|e| fit_label(&v.name(e.0), 28, 1)).collect();
        let x0 = 8 + names.iter().map(|n| label_width(n, 1)).max().unwrap_or(0);
        let x1 = cv.w - 6;
        for (i, &(p, n)) in self.progress.iter().enumerate() {
            let y = top + i as i32 * lane_h;
            let mine = p == v.me || p == v.cam;
            let lane = if i % 2 == 0 { Rgb::hex(0x16150F) } else { Rgb::hex(0x1C1A13) };
            cv.rect(0, y, cv.w, lane_h, if mine { lane.lerp(v.hue(p), 0.12) } else { lane });
            for x in (x0..x1).step_by(4) {
                cv.set(x, y + lane_h - 1, Rgb::hex(0x2C291F));
            }
            // Finish line checker.
            for yy in y..y + lane_h {
                for xx in x1..x1 + 2 {
                    cv.set(xx, yy, if (xx + yy) % 2 == 0 { pal::INK } else { pal::BLACK });
                }
            }
            let pos = l.race.iter().find(|e| e.0 == p).map(|e| e.1).unwrap_or(0.0);
            let rx = x0 + (pos * (x1 - x0 - 6) as f32) as i32;
            let c = v.hue(p);
            let bob = ((v.t * 14.0 + i as f32).sin() > 0.0) as i32;
            for (sy, row) in [".###.", "#####", "#.#.#"].iter().enumerate() {
                for (sx, ch) in row.bytes().enumerate() {
                    if ch == b'#' {
                        cv.set(rx + sx as i32, y + 1 + sy as i32 + bob, if sy == 2 { pal::INK } else { c });
                    }
                }
            }
            if pos < n as f32 / LEN as f32 - 0.004 {
                for k in 1..4 {
                    cv.blend(rx - k * 2, y + 1, c, 0.5 / k as f32);
                }
            }
            cv.label(4, y + (lane_h - SMALL) / 2, &names[i], c, 1);
        }

        // Current word: what you have typed of it turns your color.
        let me_n = self.of(v.me);
        let me_racing = self.progress.iter().any(|e| e.0 == v.me);
        let who = if me_racing { v.me } else { v.cam };
        let idx = if who == v.me { l.word.max(me_n) as usize } else { self.of(who) as usize };
        let word_y = top + self.progress.len() as i32 * lane_h + 4;
        let typo = v.t - l.typo_t < 0.18;
        let mut scale = 2;
        let cx = cv.w / 2;
        if let Some(w) = self.words.get(idx) {
            let up = w.to_uppercase();
            while scale > 1 && (big_width(&up, scale) > cv.w - 4 || word_y + type_h((7 * scale) as f32) + 2 + LINE > cv.h) {
                scale -= 1;
            }
            let jitter = if typo && !v.calm { ((v.t * 90.0).sin() * 2.0) as i32 } else { 0 };
            let typed = if who == v.me { l.typed.len().min(up.len()) } else { 0 };
            let x = cx - big_width(&up, scale) / 2 + jitter;
            cv.big(x, word_y, &up, scale, &|_, _| if typo { pal::RED } else { pal::INK });
            if !typo && typed > 0 {
                let hc = v.hue(who);
                cv.big(x, word_y, &up[..typed], scale, &|_, _| hc);
            }
        }
        let row = word_y + type_h((7 * scale) as f32) + 2;
        let preview: Vec<&str> = self.words.iter().skip(idx + 1).take(4).map(|s| s.as_str()).collect();
        if let Some(place) = self.finished.iter().position(|&p| p == who) {
            cv.heading(cx, row, &format!("FINISHED · PLACE {}", place + 1), pal::GOLD, cv.w - 8);
        } else if !preview.is_empty() && row + SMALL < cv.h {
            cv.label_center(cx, row, &fit_label(&format!("NEXT   {}", preview.join("  ")), cv.w - 8, 1), pal::DIM, 1);
        }
        cv.blit(buf, a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_a_word_reports_it_and_typos_are_rejected() {
        let s = S::new(&[1], 3, 0);
        let mut l = Local::default();
        let w = s.words[0].clone();
        assert!(matches!(s.key(&mut l, 1, '#'), Key::Typo));
        let mut last = Key::Ignore;
        for c in w.chars() {
            last = s.key(&mut l, 1, c);
        }
        assert!(matches!(last, Key::Word(Input::Word(0))));
        assert_eq!(l.word, 1);
    }

    #[test]
    fn race_ends_by_timeout_with_everyone_ranked() {
        let mut s = S::new(&[1, 2], 3, 0);
        let mut ev = vec![];
        s.input(2, &Input::Word(0), 10, &mut ev);
        assert_eq!(s.tick(LIMIT), Some(vec![2, 1]));
    }
}
