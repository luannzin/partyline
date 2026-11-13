//! GRIDLOCK: light-trail arena. Host steps every bike on a grid; last one moving wins.
use ratatui::{buffer::Buffer, layout::Rect};
use serde::{Deserialize, Serialize};

use super::{Fx, Local, View, stage};
use crate::gfx::{Canvas, Rgb, ctext, hash, pal, text};
use crate::proto::{Dir, Event, Input, Pid};

pub const W: i32 = 84;
pub const H: i32 = 44;
const WALL: u8 = 255;
const SHRINK_AFTER: u64 = 22_000;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Bike {
    pub p: Pid,
    pub x: i16,
    pub y: i16,
    pub dir: Dir,
    queue: Vec<Dir>,
    pub alive: bool,
}

// ponytail: full grid ships every frame (~3.7 KB); delta-encode trails if relay bandwidth hurts.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct S {
    pub grid: Vec<u8>,
    pub bikes: Vec<Bike>,
    next_step: u64,
    start: u64,
    shrink: i16,
    next_shrink: u64,
    pub deaths: Vec<Pid>,
    steps: u32,
}

fn idx(x: i32, y: i32) -> usize {
    (y * W + x) as usize
}

impl S {
    pub fn new(players: &[Pid], _seed: u64, now: u64) -> S {
        let mut grid = vec![0u8; (W * H) as usize];
        let n = players.len().max(1) as f32;
        let bikes = players
            .iter()
            .enumerate()
            .map(|(i, &p)| {
                let a = i as f32 / n * std::f32::consts::TAU + std::f32::consts::FRAC_PI_4;
                let (rx, ry) = (W as f32 / 2.0 - 9.0, H as f32 / 2.0 - 7.0);
                let x = (W as f32 / 2.0 + rx * a.cos()) as i32;
                let y = (H as f32 / 2.0 + ry * a.sin()) as i32;
                let (dx, dy) = ((W / 2 - x) as f32 / rx, (H / 2 - y) as f32 / ry);
                let dir = if dx.abs() > dy.abs() {
                    if dx > 0.0 { Dir::Right } else { Dir::Left }
                } else if dy > 0.0 {
                    Dir::Down
                } else {
                    Dir::Up
                };
                grid[idx(x, y)] = p + 1;
                Bike { p, x: x as i16, y: y as i16, dir, queue: vec![], alive: true }
            })
            .collect();
        S { grid, bikes, next_step: now + 250, start: now, shrink: 0, next_shrink: 0, deaths: vec![], steps: 0 }
    }

    fn step_ms(&self, now: u64) -> u64 {
        (62.0 - (now - self.start) as f32 / 1000.0 * 0.8).max(36.0) as u64
    }

    fn free(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < W && y < H && self.grid[idx(x, y)] == 0
    }

    pub fn input(&mut self, p: Pid, i: &Input) {
        let Input::Dir(d) = *i else { return };
        let Some(b) = self.bikes.iter_mut().find(|b| b.p == p && b.alive) else { return };
        let last = *b.queue.last().unwrap_or(&b.dir);
        if b.queue.len() < 2 && d != last && d != last.opposite() {
            b.queue.push(d);
        }
    }

    fn kill(&mut self, i: usize, ev: &mut Vec<Event>) {
        if self.bikes[i].alive {
            self.bikes[i].alive = false;
            self.deaths.push(self.bikes[i].p);
            ev.push(Event::Crash(self.bikes[i].p));
        }
    }

    fn step(&mut self, ev: &mut Vec<Event>) {
        self.steps += 1;
        let mut next = vec![None; self.bikes.len()];
        for (i, b) in self.bikes.iter_mut().enumerate().filter(|(_, b)| b.alive) {
            if !b.queue.is_empty() {
                b.dir = b.queue.remove(0);
            }
            let (dx, dy) = b.dir.delta();
            next[i] = Some((b.x as i32 + dx, b.y as i32 + dy));
        }
        let dead: Vec<usize> = (0..self.bikes.len())
            .filter(|&i| {
                let Some((x, y)) = next[i] else { return false };
                !self.free(x, y) || next.iter().enumerate().any(|(j, n)| j != i && *n == Some((x, y)))
            })
            .collect();
        for &i in &dead {
            self.kill(i, ev);
        }
        for (i, b) in self.bikes.iter_mut().enumerate() {
            if let (true, Some((x, y))) = (b.alive, next[i]) {
                self.grid[idx(x, y)] = b.p + 1;
                (b.x, b.y) = (x as i16, y as i16);
            }
        }
    }

    pub fn tick(&mut self, now: u64, ev: &mut Vec<Event>) -> Option<Vec<Pid>> {
        while now >= self.next_step {
            self.step(ev);
            self.next_step += self.step_ms(now);
        }
        if now >= self.start + SHRINK_AFTER && now >= self.next_shrink && self.shrink < H as i16 / 2 - 3 {
            let k = self.shrink as i32;
            for x in k..W - k {
                for y in [k, H - 1 - k] {
                    self.grid[idx(x, y)] = WALL;
                }
            }
            for y in k..H - k {
                for x in [k, W - 1 - k] {
                    self.grid[idx(x, y)] = WALL;
                }
            }
            self.shrink += 1;
            self.next_shrink = now + 900;
            for i in 0..self.bikes.len() {
                let b = &self.bikes[i];
                if self.grid[idx(b.x as i32, b.y as i32)] == WALL {
                    self.kill(i, ev);
                }
            }
        }
        let alive = self.bikes.iter().filter(|b| b.alive).count();
        let over = if self.bikes.len() > 1 { alive <= 1 } else { alive == 0 };
        over.then(|| {
            let mut rank: Vec<Pid> = self.bikes.iter().filter(|b| b.alive).map(|b| b.p).collect();
            rank.extend(self.deaths.iter().rev());
            rank
        })
    }

    fn flood(&self, x: i32, y: i32, cap: usize) -> usize {
        if !self.free(x, y) {
            return 0;
        }
        let mut seen = vec![false; (W * H) as usize];
        let mut q = vec![(x, y)];
        seen[idx(x, y)] = true;
        let mut n = 0;
        while let Some((cx, cy)) = q.pop() {
            n += 1;
            if n >= cap {
                break;
            }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (cx + dx, cy + dy);
                if self.free(nx, ny) && !seen[idx(nx, ny)] {
                    seen[idx(nx, ny)] = true;
                    q.push((nx, ny));
                }
            }
        }
        n
    }

    pub fn bot(&self, me: Pid) -> Option<Input> {
        let b = self.bikes.iter().find(|b| b.p == me && b.alive)?;
        if !b.queue.is_empty() {
            return None;
        }
        let turns = match b.dir {
            Dir::Up | Dir::Down => [b.dir, Dir::Left, Dir::Right],
            _ => [b.dir, Dir::Up, Dir::Down],
        };
        let score = |d: Dir| {
            let (dx, dy) = d.delta();
            let (x, y) = (b.x as i32 + dx, b.y as i32 + dy);
            // Look two cells ahead so bots don't hug walls until the last moment.
            let ahead = self.free(x + dx, y + dy) as usize * 8;
            self.flood(x, y, 220) * 4 + ahead
        };
        let wiggle = hash(self.steps, me as u32) % 37 == 0;
        let mut best = turns[0];
        let mut best_s = score(best) + if wiggle { 0 } else { 30 };
        for &d in &turns[1..] {
            let s = score(d);
            if s > best_s {
                (best, best_s) = (d, s);
            }
        }
        (best != b.dir).then_some(Input::Dir(best))
    }

    /// Arena placement inside the game area: (scale, origin x, origin y) in canvas pixels.
    fn layout(area: Rect) -> (i32, i32, i32) {
        let (cw, ch) = (area.width as i32, area.height as i32 * 2);
        let s = ((cw - 2) / W).min((ch - 6) / H).max(1);
        ((s), (cw - W * s) / 2, (ch - H * s) / 2 + 2)
    }

    pub fn on_event(&self, e: Event, v: &View, l: &mut Local, fx: &mut Fx) {
        if let Event::Crash(p) = e {
            l.crashed.push((p, v.t));
            if let Some(b) = self.bikes.iter().find(|b| b.p == p) {
                let (s, ox, oy) = Self::layout(v.area);
                let (x, y) = ((ox + b.x as i32 * s) as f32, (oy + b.y as i32 * s) as f32);
                l.burst(x, y, v.hue(p), 34, 60.0, p as u32 * 31 + self.steps);
                l.burst(x, y, pal::INK, 10, 40.0, p as u32 * 17 + self.steps);
            }
            let mine = p == v.me;
            if !v.calm {
                fx.kick(if mine { 1.5 } else { 0.6 }, if mine { 0.5 } else { 0.15 }, if mine { pal::RED } else { pal::INK });
            }
        }
    }

    pub fn render(&self, v: &View, l: &mut Local, buf: &mut Buffer) {
        let a = v.area;
        let mut cv: Canvas = stage(a, v.t, 0.3);
        let (s, ox, oy) = Self::layout(a);
        let focus = (v.cam != v.me).then_some(v.cam);
        let crash_t = |p: Pid| l.crashed.iter().find(|c| c.0 == p).map(|c| c.1);

        // Arena floor and wall glow.
        cv.rect(ox - 1, oy - 1, W * s + 2, H * s + 2, pal::GOLD.mul(0.9));
        for y in 0..H {
            for x in 0..W {
                let cell = self.grid[idx(x, y)];
                let c = match cell {
                    0 if x % 4 == 0 && y % 4 == 0 => Rgb::hex(0x24221B),
                    0 => Rgb::hex(0x141310),
                    WALL => if (x + y) % 4 < 2 { pal::PODIUM } else { pal::PODIUM_DARK },
                    owner => {
                        let p = owner - 1;
                        let mut c = v.hue(p).mul(0.72);
                        if let Some(t0) = crash_t(p) {
                            c = c.lerp(c.grey().mul(0.6), (v.t - t0) / 0.6);
                        }
                        if focus.is_some_and(|f| f != p) {
                            c = c.mul(0.45);
                        }
                        c
                    }
                };
                cv.rect(ox + x * s, oy + y * s, s, s, c);
            }
        }
        for b in self.bikes.iter().filter(|b| b.alive) {
            let (hx, hy) = ((ox + b.x as i32 * s) as f32 + s as f32 / 2.0, (oy + b.y as i32 * s) as f32 + s as f32 / 2.0);
            cv.light(hx, hy, 4.0 * s as f32, v.hue(b.p), 0.45);
            cv.rect(ox + b.x as i32 * s, oy + b.y as i32 * s, s, s, v.hue(b.p).lerp(pal::INK, 0.6));
        }
        let dt = (v.t - l.pt).clamp(0.0, 0.1);
        l.pt = v.t;
        l.draw_parts(&mut cv, dt, 40.0);
        cv.blit(buf, a);

        // Side legend when the arena leaves room for it.
        if ox >= 16 {
            for (i, b) in self.bikes.iter().enumerate() {
                let y = a.y as i32 + oy / 2 + i as i32 * 2;
                let c = if b.alive { v.hue(b.p) } else { v.hue(b.p).grey() };
                text(buf, a, a.x as i32 + 3, y, &v.name(b.p), c);
                text(buf, a, a.x as i32 + 3, y + 1, if b.alive { "moving" } else { "crashed" }, if b.alive { pal::DIM } else { pal::RED.mul(0.8) });
            }
        }

        // Labels.
        let alive = self.bikes.iter().filter(|b| b.alive).count();
        let label = format!("{alive} OF {} STILL MOVING", self.bikes.len());
        ctext(buf, a, a.y as i32, &label, pal::DIM);
        if let Some(me) = self.bikes.iter().find(|b| b.p == v.me) {
            if !me.alive {
                ctext(buf, a, a.bottom() as i32 - 1, "you crashed - watch who survives", pal::RED);
            } else if self.steps < 30 {
                let (x, y) = (a.x as i32 + ox + me.x as i32 * s, a.y as i32 + (oy + me.y as i32 * s) / 2 - 1);
                text(buf, a, x - 1, y, "YOU", pal::INK);
            }
        }
        if self.shrink > 0 && self.shrink < 3 {
            ctext(buf, a, a.bottom() as i32 - 1, "the walls are closing in", pal::GOLD);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bots_play_to_a_full_ranking() {
        let mut s = S::new(&[1, 2, 3, 4], 1, 0);
        let mut ev = vec![];
        let mut ranking = None;
        let mut now = 0;
        while ranking.is_none() && now < 200_000 {
            now += 16;
            for p in 1..=4 {
                if let Some(i) = s.bot(p) {
                    s.input(p, &i);
                }
            }
            ranking = s.tick(now, &mut ev);
        }
        let mut r = ranking.expect("game must end");
        assert_eq!(r.len(), 4);
        r.sort();
        assert_eq!(r, vec![1, 2, 3, 4]);
    }

    #[test]
    fn reversing_is_ignored() {
        let mut s = S::new(&[1], 1, 0);
        let d = s.bikes[0].dir;
        s.input(1, &Input::Dir(d.opposite()));
        assert!(s.bikes[0].queue.is_empty());
    }
}
