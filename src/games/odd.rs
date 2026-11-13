//! ODD ONE OUT: everyone secretly picks one of three answers; the least-picked answer scores.
use ratatui::buffer::Buffer;
use serde::{Deserialize, Serialize};

use super::{Fx, Local, View, panel, stage};
use crate::gfx::Canvas;
use crate::gfx::{LINE, Rgb, SMALL, big_width, ease_out, fit_label, hash, label_width, pal, prog, type_h, wrap_label};

/// A key or chip: the small face on a plate.
const CHIP: i32 = SMALL + 2;
use crate::proto::{Event, Input, Pid};

pub const ROUNDS: u8 = 5;
const PICK_MS: u64 = 7000;
const REVEAL_MS: u64 = 4200;

const QS: &[(&str, [&str; 3])] = &[
    ("PICK A DOOR", ["RED", "GREEN", "BLUE"]),
    ("BEST PIZZA", ["CHEESE", "PEPPERONI", "PINEAPPLE"]),
    ("SUPERPOWER", ["FLIGHT", "INVISIBLE", "TELEPORT"]),
    ("WEEKEND", ["PARTY", "SLEEP", "GAMES"]),
    ("PET", ["CAT", "DOG", "DRAGON"]),
    ("TRAVEL BY", ["TRAIN", "PLANE", "ROCKET"]),
    ("SEASON", ["SUMMER", "WINTER", "AUTUMN"]),
    ("MUSIC", ["ROCK", "FUNK", "TECHNO"]),
    ("A NUMBER", ["ONE", "TWO", "THREE"]),
    ("VILLAIN LAIR", ["VOLCANO", "MOON", "OFFICE"]),
    ("DESSERT", ["CAKE", "ICE CREAM", "FRUIT"]),
    ("BREAKFAST", ["COFFEE", "JUICE", "NOTHING"]),
    ("A SHAPE", ["CIRCLE", "SQUARE", "TRIANGLE"]),
    ("HIDING SPOT", ["CLOSET", "ROOF", "FRIDGE"]),
];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Stage {
    Pick { until: u64 },
    Reveal { until: u64 },
}

// ponytail: picks travel to every client during the pick window; trust model is friends on a couch.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct S {
    pub qs: Vec<u8>,
    pub round: u8,
    pub stage: Stage,
    pub picks: Vec<(Pid, Option<u8>)>,
    pub points: Vec<(Pid, u8)>,
    pub winners: Vec<u8>,
    seed: u64,
}

impl S {
    pub fn new(players: &[Pid], seed: u64, now: u64) -> S {
        let mut qs: Vec<u8> = (0..QS.len() as u8).collect();
        qs.sort_by_key(|&i| hash(seed as u32, i as u32));
        qs.truncate(ROUNDS as usize);
        S {
            qs,
            round: 0,
            stage: Stage::Pick { until: now + PICK_MS },
            picks: players.iter().map(|&p| (p, None)).collect(),
            points: players.iter().map(|&p| (p, 0)).collect(),
            winners: vec![],
            seed,
        }
    }

    fn q(&self) -> &(&'static str, [&'static str; 3]) {
        &QS[self.qs[self.round.min(ROUNDS - 1) as usize] as usize]
    }

    pub fn input(&mut self, p: Pid, i: &Input, ev: &mut Vec<Event>) {
        let (Input::Pick(k), Stage::Pick { .. }) = (*i, &self.stage) else { return };
        if k < 3 {
            if let Some(e) = self.picks.iter_mut().find(|e| e.0 == p && e.1.is_none()) {
                e.1 = Some(k);
                ev.push(Event::Locked(p));
            }
        }
    }

    pub fn tick(&mut self, now: u64, ev: &mut Vec<Event>) -> Option<Vec<Pid>> {
        match self.stage {
            Stage::Pick { until } if now >= until || self.picks.iter().all(|e| e.1.is_some()) => {
                let mut count = [0usize; 3];
                for k in self.picks.iter().filter_map(|e| e.1) {
                    count[k as usize] += 1;
                }
                let used: Vec<usize> = count.iter().copied().filter(|&c| c > 0).collect();
                let (lo, hi) = (used.iter().min().copied().unwrap_or(0), used.iter().max().copied().unwrap_or(0));
                self.winners = (0..3u8).filter(|&k| lo < hi && count[k as usize] == lo).collect();
                for (p, k) in &self.picks {
                    if k.is_some_and(|k| self.winners.contains(&k)) {
                        if let Some(e) = self.points.iter_mut().find(|e| e.0 == *p) {
                            e.1 += 1;
                        }
                    }
                }
                self.stage = Stage::Reveal { until: now + REVEAL_MS };
                ev.push(Event::Reveal);
            }
            Stage::Reveal { until } if now >= until => {
                self.round += 1;
                if self.round >= ROUNDS {
                    let mut rank = self.points.clone();
                    rank.sort_by_key(|e| std::cmp::Reverse(e.1));
                    return Some(rank.into_iter().map(|e| e.0).collect());
                }
                for e in &mut self.picks {
                    e.1 = None;
                }
                self.winners.clear();
                self.stage = Stage::Pick { until: now + PICK_MS };
            }
            _ => {}
        }
        None
    }

    pub fn bot(&self, me: Pid, now: u64) -> Option<Input> {
        let Stage::Pick { until } = self.stage else { return None };
        if self.picks.iter().find(|e| e.0 == me)?.1.is_some() {
            return None;
        }
        let h = hash(self.seed as u32 ^ me as u32, self.round as u32);
        (now + PICK_MS >= until + 1200 + (h % 3500) as u64).then_some(Input::Pick((h >> 8) as u8 % 3))
    }

    pub fn on_event(&self, e: Event, v: &View, l: &mut Local, fx: &mut Fx) {
        if e == Event::Reveal {
            l.reveal_t = v.t;
            let mine = self.picks.iter().find(|p| p.0 == v.me).and_then(|p| p.1);
            if mine.is_some_and(|k| self.winners.contains(&k)) && !v.calm {
                fx.kick(0.6, 0.35, pal::GOLD);
            }
        }
    }

    /// Narrow screens: the question as a heading and the answers as rows, key, answer and (once
    /// revealed) how many picked it; the clock runs along the bottom.
    fn render_list(&self, v: &View, cv: &mut Canvas, my: Option<u8>, rt: f32) {
        let (prompt, opts) = self.q();
        let reveal = matches!(self.stage, Stage::Reveal { .. });
        let (cx, rw) = (cv.w / 2, cv.w - 8);
        cv.heading(cx, 2, prompt, pal::BULB, rw);
        let pitch = ((cv.h - 4 - LINE - 4) / 3).clamp(CHIP + 1, 12);
        for k in 0..3u8 {
            let y = 2 + LINE + 3 + k as i32 * pitch;
            let won = reveal && self.winners.contains(&k);
            let lost = reveal && !self.winners.is_empty() && !won;
            let mut face = pal::PANEL;
            if my == Some(k) {
                face = face.lerp(pal::GOLD, 0.18);
            }
            if lost {
                face = face.mul(0.6);
            }
            if won {
                cv.light(cx as f32, (y + pitch / 2) as f32, rw as f32 * 0.6, pal::GOLD, 0.3 * (0.8 + 0.2 * (v.t * 6.0).sin()) * (rt * 4.0).min(1.0));
            }
            cv.rect(4, y, rw, pitch - 1, face);
            cv.rect(4, y, 2, pitch - 1, if my == Some(k) || won { pal::GOLD } else { pal::DIM.mul(0.6) });
            let ty = y + (pitch - 1 - CHIP) / 2;
            cv.chip(8, ty, &(k + 1).to_string(), pal::STAGE_DEEP, pal::INK.mul(0.78));
            let mut right = 4 + rw - 3;
            if reveal {
                let n = self.picks.iter().filter(|p| p.1 == Some(k)).count();
                let count = if won { format!("★ {n}") } else { n.to_string() };
                right -= label_width(&count, 1);
                cv.label(right, ty + 1, &count, if won { pal::GOLD } else { pal::DIM }, 1);
                right -= 4;
            }
            cv.label(8 + CHIP + 4, ty + 1, &fit_label(opts[k as usize], right - (8 + CHIP + 4), 1), pal::INK, 1);
        }
        if let Stage::Pick { until } = self.stage {
            let left = until.saturating_sub(v.now) as f32 / PICK_MS as f32;
            cv.rect(4, cv.h - 2, ((cv.w - 8) as f32 * left) as i32, 1, if left < 0.3 { pal::RED } else { pal::GOLD });
        }
    }

    pub fn render(&self, v: &View, l: &mut Local, buf: &mut Buffer) {
        let a = v.area;
        let reveal = matches!(self.stage, Stage::Reveal { .. });
        let mut cv = stage(a, v.t, if reveal { 0.6 } else { 0.1 });
        let (prompt, opts) = self.q();
        let my = self.picks.iter().find(|p| p.0 == v.me).and_then(|p| p.1);
        let rt = v.t - l.reveal_t;
        if (cv.w - 6) / 3 < 34 {
            self.render_list(v, &mut cv, my, rt);
            return cv.blit(buf, a);
        }
        let mut scale = if cv.h >= 80 { 2 } else { 1 };
        while scale > 1 && big_width(prompt, scale) > cv.w - 4 {
            scale -= 1;
        }
        let cx = cv.w / 2;
        cv.big_center(cx, 3, prompt, scale, &|_, y| pal::BULB.lerp(pal::ORANGE, y as f32 / (7 * scale) as f32));

        // Under the cards: the clock, then one row (two when tall) of who has locked in, how to
        // pick, or who scored.
        let rows = if cv.h >= 80 { 2 } else { 1 };
        let (cw, top) = (((cv.w - 6) / 3).min(44), 3 + type_h((7 * scale) as f32) + 3);
        let foot = cv.h - 1 - rows * (CHIP + 3);
        let ph = (foot - 3 - top).min(52);
        let x0 = (cv.w - (cw * 3 + 4)) / 2;
        for k in 0..3u8 {
            let x = x0 + k as i32 * (cw + 2);
            let won = reveal && self.winners.contains(&k);
            let lost = reveal && !self.winners.is_empty() && !won;
            let mut face = pal::PANEL;
            if my == Some(k) {
                face = face.lerp(pal::GOLD, 0.18);
            }
            if lost {
                face = face.mul(0.6);
            }
            let lift = if won { (ease_out(prog(rt, 0.2, 0.35)) * 3.0) as i32 } else { 0 };
            if won {
                cv.light((x + cw / 2) as f32, (top + ph / 2) as f32, cw as f32, pal::GOLD, 0.35 * (0.8 + 0.2 * (v.t * 6.0).sin()));
            }
            let (y, cx) = (top - lift, x + cw / 2);
            panel(&mut cv, x, y, cw, ph, face, if my == Some(k) || won { pal::GOLD } else { pal::DIM.mul(0.6) });
            // The key (and once revealed how many picked it), the answer, then who picked it
            // as far as the card has room.
            cv.chip(x + 3, y + 4, &(k + 1).to_string(), pal::STAGE_DEEP, pal::INK.mul(0.78));
            let pickers: Vec<Pid> = self.picks.iter().filter(|p| p.1 == Some(k)).map(|p| p.0).collect();
            if reveal {
                let count = if won { format!("★ {}", pickers.len()) } else { pickers.len().to_string() };
                cv.label(x + cw - 3 - label_width(&count, 1), y + 5, &count, if won { pal::GOLD } else { pal::DIM }, 1);
            }
            let mut ly = y + 4 + CHIP + 2;
            for line in wrap_label(opts[k as usize], cw - 4) {
                cv.label_center(cx, ly, &fit_label(&line, cw - 2, 1), pal::INK, 1);
                ly += LINE;
            }
            if reveal {
                ly += 1;
                for (i, p) in pickers.iter().enumerate() {
                    if ly + SMALL > y + ph - 2 {
                        break;
                    }
                    if rt > 0.25 + i as f32 * 0.07 {
                        cv.label_center(cx, ly, &fit_label(&v.name(*p), cw - 6, 1), v.hue(*p), 1);
                    }
                    ly += LINE;
                }
            }
        }

        let cx = cv.w / 2;
        let row = foot + 3;
        let ask = my.is_none() && self.picks.iter().any(|p| p.0 == v.me);
        match self.stage {
            Stage::Pick { until } => {
                let left = until.saturating_sub(v.now) as f32 / PICK_MS as f32;
                let c = if left < 0.3 { pal::RED } else { pal::GOLD };
                cv.rect(4, foot, ((cv.w - 8) as f32 * left) as i32, 1, c);
                if !ask || rows == 2 {
                    let n = self.picks.len() as i32;
                    let mut x = cx - (n * 8 - 3) / 2;
                    for (p, k) in &self.picks {
                        let c = v.hue(*p);
                        cv.rect(x, row + 1, 5, 5, if k.is_some() { c } else { c.mul(0.25) });
                        x += 8;
                    }
                }
                if ask {
                    cv.keys(cx, foot + rows * (CHIP + 3) - CHIP, cv.w - 8, &[("1 2 3", "pick · it locks in")], v.t);
                }
            }
            Stage::Reveal { .. } if self.winners.is_empty() => cv.label_center(cx, row + 1, "EVERYONE AGREED · NO POINTS", pal::DIM, 1),
            Stage::Reveal { .. } => {
                // Points so far in this game, as many chips as fit.
                let chips: Vec<(String, Rgb)> = self.points.iter().map(|(p, n)| (format!("{} {n}", fit_label(&v.name(*p), 24, 1)), v.hue(*p))).collect();
                let (mut n, mut w) = (0, -3);
                for (s, _) in &chips {
                    let cw = label_width(s, 1) + 7;
                    if w + cw > cv.w - 8 {
                        break;
                    }
                    (n, w) = (n + 1, w + cw);
                }
                let mut x = cx - w / 2;
                for (s, c) in &chips[..n] {
                    x += cv.chip(x, row, s, pal::STAGE_DEEP, *c) + 3;
                }
            }
        }
        cv.blit(buf, a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minority_scores_and_unanimous_scores_nothing() {
        let mut s = S::new(&[1, 2, 3], 5, 0);
        let mut ev = vec![];
        s.input(1, &Input::Pick(0), &mut ev);
        s.input(2, &Input::Pick(0), &mut ev);
        s.input(3, &Input::Pick(2), &mut ev);
        s.tick(1, &mut ev);
        assert_eq!(s.winners, vec![2]);
        assert_eq!(s.points.iter().find(|e| e.0 == 3).unwrap().1, 1);

        s.tick(REVEAL_MS + 2, &mut ev);
        for p in 1..=3 {
            s.input(p, &Input::Pick(1), &mut ev);
        }
        s.tick(REVEAL_MS + 3, &mut ev);
        assert!(s.winners.is_empty());
    }
}
