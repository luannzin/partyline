//! BLINK: reaction duel. Reaction time is measured on each client from the moment its own
//! screen showed the flash, so network latency never decides the winner.
use std::time::Instant;

use ratatui::buffer::Buffer;
use serde::{Deserialize, Serialize};

use super::{Fx, Local, View, panel, stage};
use crate::gfx::{Rgb, big_width, ctext, hash, pal, text};
use crate::proto::{Event, Input, Pid};

pub const FLASHES: u8 = 3;
const MISS: u16 = 1000;
const EARLY: u16 = 1001;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Stage {
    Wait { flash_at: u64 },
    Flash { at: u64 },
    Show { until: u64 },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct S {
    pub round: u8,
    pub stage: Stage,
    pub rt: Vec<(Pid, Vec<Option<u16>>)>,
    seed: u64,
}

fn wait_ms(seed: u64, round: u8) -> u64 {
    1700 + (hash(seed as u32, round as u32) % 2600) as u64
}

impl S {
    pub fn new(players: &[Pid], seed: u64, now: u64) -> S {
        S {
            round: 0,
            stage: Stage::Wait { flash_at: now + wait_ms(seed, 0) },
            rt: players.iter().map(|&p| (p, vec![None; FLASHES as usize])).collect(),
            seed,
        }
    }

    fn r(&self) -> usize {
        self.round.min(FLASHES - 1) as usize
    }

    fn total(&self, p: Pid) -> u32 {
        self.rt.iter().find(|e| e.0 == p).map(|e| e.1.iter().flatten().map(|&v| v as u32).sum()).unwrap_or(u32::MAX)
    }

    pub fn input(&mut self, p: Pid, i: &Input, ev: &mut Vec<Event>) {
        let (r, stage) = (self.r(), self.stage.clone());
        let Some(slot) = self.rt.iter_mut().find(|e| e.0 == p).map(|e| &mut e.1[r]) else { return };
        if slot.is_some() {
            return;
        }
        match (stage, i) {
            (Stage::Wait { .. } | Stage::Flash { .. }, Input::Early) => {
                *slot = Some(EARLY);
                ev.push(Event::FalseStart(p));
            }
            (Stage::Flash { .. }, Input::React(ms)) => {
                *slot = Some((*ms).min(MISS));
                ev.push(Event::Reacted(p));
            }
            _ => {}
        }
    }

    pub fn tick(&mut self, now: u64, ev: &mut Vec<Event>) -> Option<Vec<Pid>> {
        let r = self.r();
        match self.stage {
            Stage::Wait { flash_at } if now >= flash_at => {
                self.stage = Stage::Flash { at: now };
                ev.push(Event::Flash);
            }
            Stage::Flash { at } => {
                if self.rt.iter().all(|e| e.1[r].is_some()) || now >= at + 1500 {
                    for e in &mut self.rt {
                        e.1[r].get_or_insert(MISS);
                    }
                    self.stage = Stage::Show { until: now + 2000 };
                }
            }
            Stage::Show { until } if now >= until => {
                self.round += 1;
                if self.round >= FLASHES {
                    let mut rank: Vec<Pid> = self.rt.iter().map(|e| e.0).collect();
                    rank.sort_by_key(|&p| self.total(p));
                    return Some(rank);
                }
                self.stage = Stage::Wait { flash_at: now + wait_ms(self.seed, self.round) };
            }
            _ => {}
        }
        None
    }

    pub fn bot(&self, me: Pid, now: u64) -> Option<Input> {
        let r = self.r();
        if self.rt.iter().find(|e| e.0 == me)?.1[r].is_some() {
            return None;
        }
        let h = hash(self.seed as u32 ^ (me as u32 * 7919), self.round as u32);
        match self.stage {
            Stage::Wait { flash_at } => (h % 19 == 0 && now + 260 >= flash_at).then_some(Input::Early),
            Stage::Flash { at } => {
                let rt = 180 + (h % 190) as u64;
                (now >= at + rt).then_some(Input::React(rt as u16))
            }
            Stage::Show { .. } => None,
        }
    }

    pub fn observe(&self, l: &mut Local) {
        if matches!(self.stage, Stage::Flash { .. }) && l.seen.map(|s| s.0) != Some(self.round) {
            l.seen = Some((self.round, Instant::now()));
        }
    }

    /// Local key press: decide early vs reaction using this client's own clock.
    pub fn press(&self, l: &mut Local) -> Option<Input> {
        if l.pressed == Some(self.round) {
            return None;
        }
        let input = match self.stage {
            Stage::Wait { .. } => Input::Early,
            Stage::Flash { .. } => {
                let ms = l.seen.filter(|s| s.0 == self.round).map(|s| s.1.elapsed().as_millis() as u16);
                Input::React(ms.unwrap_or(MISS).min(MISS))
            }
            Stage::Show { .. } => return None,
        };
        l.pressed = Some(self.round);
        Some(input)
    }

    pub fn on_event(&self, e: Event, v: &View, _l: &mut Local, fx: &mut Fx) {
        match e {
            Event::Flash => fx.kick(if v.calm { 0.0 } else { 1.4 }, if v.calm { 0.35 } else { 0.95 }, pal::INK),
            Event::FalseStart(p) if p == v.me => fx.kick(if v.calm { 0.0 } else { 0.7 }, 0.4, pal::RED),
            _ => {}
        }
    }

    pub fn render(&self, v: &View, _l: &mut Local, buf: &mut Buffer) {
        let a = v.area;
        let flashing = matches!(self.stage, Stage::Flash { .. });
        let mut cv = stage(a, v.t, if flashing { 1.0 } else { 0.0 });
        let r = self.r();

        // Big message.
        let my = self.rt.iter().find(|e| e.0 == v.me).and_then(|e| e.1[r]);
        let (msg, scale_cap, fill): (String, i32, Box<dyn Fn(i32, i32) -> Rgb>) = match self.stage {
            Stage::Wait { .. } if my == Some(EARLY) => ("TOO EARLY!".into(), 2, Box::new(|_, _| pal::RED)),
            Stage::Wait { .. } => {
                let k = 0.5 + 0.5 * (v.t * 3.2).sin();
                let c = pal::DIM.lerp(pal::BULB_OFF.lerp(pal::GOLD, 0.35), k);
                ("WAIT FOR IT".into(), 2, Box::new(move |_, _| c))
            }
            Stage::Flash { .. } => ("NOW!".into(), 5, Box::new(|_, y| pal::INK.lerp(pal::GOLD, y as f32 / 30.0))),
            Stage::Show { .. } => {
                let best = self.rt.iter().filter_map(|e| e.1[r].map(|t| (t, e.0))).filter(|x| x.0 < MISS).min();
                match best {
                    Some((_, p)) => {
                        let c = v.hue(p);
                        (v.name(p), 2, Box::new(move |_, _| c))
                    }
                    None => ("NOBODY!".into(), 2, Box::new(|_, _| pal::DIM)),
                }
            }
        };
        // Player tiles anchor to the bottom; the message gets the space above them.
        let n = self.rt.len().max(1) as i32;
        let cols = n.min(4);
        let tw = ((a.width as i32 - 4) / cols - 2).min(22);
        let th = 5;
        let rows = (n + cols - 1) / cols;
        let top = a.height as i32 - rows * (th + 1) - 2;
        let avail = top * 2 - 10;
        let mut scale = scale_cap;
        while scale > 1 && (big_width(&msg, scale) > cv.w - 4 || 7 * scale > avail) {
            scale -= 1;
        }
        let (my_y, cx) = (6 + (avail - 7 * scale) / 2, cv.w / 2);
        cv.big_center(cx, my_y, &msg, scale, &*fill);
        let best = self.rt.iter().filter_map(|e| e.1[r]).filter(|&t| t < MISS).min();
        let mut labels = Vec::new();
        for (i, (p, times)) in self.rt.iter().enumerate() {
            let (col, row) = (i as i32 % cols, i as i32 / cols);
            let row_n = (n - row * cols).min(cols);
            let x = (a.width as i32 - row_n * (tw + 2)) / 2 + col * (tw + 2) + 1;
            let y = top + row * (th + 1);
            let early = times[r] == Some(EARLY);
            let mut stripe = v.hue(*p);
            let mut face = Rgb::hex(0x151C6E);
            if early {
                stripe = stripe.grey();
                face = face.grey();
            }
            if v.cam != v.me && *p != v.cam {
                face = face.mul(0.75);
            }
            panel(&mut cv, x, y * 2, tw, th * 2, face, stripe);
            let status = match times[r] {
                None if flashing => ("· · ·".to_string(), pal::GOLD),
                None => ("ready".to_string(), pal::DIM),
                Some(EARLY) => ("TOO EARLY".to_string(), pal::RED),
                Some(MISS) => ("MISSED".to_string(), pal::DIM),
                Some(t) if Some(t) == best && matches!(self.stage, Stage::Show { .. }) => (format!("* {t} MS"), pal::GOLD),
                Some(t) => (format!("{t} MS"), pal::INK),
            };
            let total: u32 = times[..r].iter().flatten().map(|&t| t as u32).sum();
            labels.push((x, y, v.name(*p), status, total, early));
        }
        cv.blit(buf, a);
        for (x, y, name, (status, sc), total, early) in labels {
            let (x, y) = (a.x as i32 + x, a.y as i32 + y);
            text(buf, a, x + 1, y + 1, &name, if early { pal::DIM } else { pal::INK });
            text(buf, a, x + 1, y + 2, &status, sc);
            if r > 0 {
                text(buf, a, x + 1, y + 3, &format!("total {total}"), pal::DIM);
            }
        }
        let label = format!("FLASH {} OF {}", r + 1, FLASHES);
        ctext(buf, a, a.y as i32 + 1, &label, pal::DIM);
        if matches!(self.stage, Stage::Wait { .. }) && my.is_none() {
            ctext(buf, a, a.bottom() as i32 - 2, "press SPACE the instant the screen flashes", pal::DIM);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fastest_total_wins_and_false_start_is_penalised() {
        let mut s = S::new(&[1, 2], 9, 0);
        let mut ev = vec![];
        let mut now = 0;
        let mut ranking = None;
        while ranking.is_none() && now < 60_000 {
            now += 16;
            if matches!(s.stage, Stage::Wait { .. }) {
                s.input(2, &Input::Early, &mut ev);
            }
            if matches!(s.stage, Stage::Flash { .. }) {
                s.input(1, &Input::React(250), &mut ev);
            }
            ranking = s.tick(now, &mut ev);
        }
        assert_eq!(ranking, Some(vec![1, 2]));
        assert!(ev.contains(&Event::FalseStart(2)));
    }
}
