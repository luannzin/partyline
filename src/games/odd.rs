//! ODD ONE OUT: everyone secretly picks one of three answers; the least-picked answer scores.
use ratatui::buffer::Buffer;
use serde::{Deserialize, Serialize};

use super::{Fx, Local, View, panel, stage};
use crate::gfx::{Rgb, big_width, ctext, ease_out, hash, pal, prog, text};
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
    ("MUSIC", ["ROCK", "FUNK", "ELECTRONIC"]),
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

    pub fn render(&self, v: &View, l: &mut Local, buf: &mut Buffer) {
        let a = v.area;
        let reveal = matches!(self.stage, Stage::Reveal { .. });
        let mut cv = stage(a, v.t, if reveal { 0.6 } else { 0.1 });
        let (prompt, opts) = self.q();
        let mut scale = 2;
        while scale > 1 && big_width(prompt, scale) > cv.w - 4 {
            scale -= 1;
        }
        let cx = cv.w / 2;
        cv.big_center(cx, 5, prompt, scale, &|_, y| pal::BULB.lerp(pal::ORANGE, y as f32 / 14.0));

        let my = self.picks.iter().find(|p| p.0 == v.me).and_then(|p| p.1);
        let (cw, top) = (((a.width as i32 - 8) / 3).min(30), 5 + 7 * scale + 8);
        let ph = (cv.h - top - 14).clamp(14, 44);
        let x0 = (cv.w - (cw * 3 + 4)) / 2;
        let rt = v.t - l.reveal_t;
        let mut labels = vec![];
        for k in 0..3u8 {
            let x = x0 + k as i32 * (cw + 2);
            let won = reveal && self.winners.contains(&k);
            let lost = reveal && !self.winners.is_empty() && !won;
            let mut face = Rgb::hex(0x151C6E);
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
            panel(&mut cv, x, top - lift, cw, ph, face, if my == Some(k) || won { pal::GOLD } else { pal::DIM.mul(0.6) });
            let pickers: Vec<Pid> = self.picks.iter().filter(|p| p.1 == Some(k)).map(|p| p.0).collect();
            labels.push((x, (top - lift) / 2, k, won, pickers));
        }
        cv.blit(buf, a);

        for (x, row, k, won, pickers) in labels {
            let (ax, ay) = (a.x as i32 + x, a.y as i32 + row);
            text(buf, a, ax + 2, ay + 1, &format!("[{}]", k + 1), pal::GOLD);
            let opt = opts[k as usize];
            text(buf, a, ax + (cw - opt.len() as i32) / 2, ay + 3, opt, pal::INK);
            if reveal {
                let n = pickers.len();
                text(buf, a, ax + (cw - 9) / 2, ay + 5, &format!("{n} picked"), if won { pal::GOLD } else { pal::DIM });
                for (i, p) in pickers.iter().enumerate() {
                    if rt > 0.25 + i as f32 * 0.07 {
                        text(buf, a, ax + 2, ay + 7 + i as i32, &v.name(*p), v.hue(*p));
                    }
                }
                if won {
                    text(buf, a, ax + (cw - 9) / 2, ay + ph / 2 - 2, "ODD WINS!", pal::GOLD);
                }
            }
        }

        let status = a.bottom() as i32 - 3;
        match self.stage {
            Stage::Pick { until } => {
                let left = until.saturating_sub(v.now) as f32 / PICK_MS as f32;
                let w = ((a.width as f32 - 8.0) * left) as usize;
                let c = if left < 0.3 { pal::RED } else { pal::GOLD };
                text(buf, a, a.x as i32 + 4, status, &"━".repeat(w), c);
                let locked: Vec<String> =
                    self.picks.iter().map(|(p, k)| format!("{}{}", v.name(*p), if k.is_some() { " ✓" } else { " …" })).collect();
                ctext(buf, a, status + 1, &locked.join("   "), pal::DIM);
                if my.is_none() && self.picks.iter().any(|p| p.0 == v.me) {
                    ctext(buf, a, status + 2, "press 1, 2 or 3 - your pick locks in", pal::INK);
                }
            }
            Stage::Reveal { .. } if self.winners.is_empty() => ctext(buf, a, status + 1, "Everyone agreed. No points.", pal::DIM),
            Stage::Reveal { .. } => {
                let pts: Vec<String> = self.points.iter().map(|(p, n)| format!("{} {n}", v.name(*p))).collect();
                ctext(buf, a, status + 1, &pts.join("   "), pal::DIM);
            }
        }
        ctext(buf, a, a.y as i32, &format!("QUESTION {} OF {ROUNDS}", self.round.min(ROUNDS - 1) + 1), pal::DIM);
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
