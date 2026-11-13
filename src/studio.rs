//! Inside the TV: the game-show studio. Marquee bulbs, contestant podiums, title cards,
//! countdown, results board and the champion's podium.
use ratatui::{buffer::Buffer, layout::Rect};

use crate::games::{self, Local, stage};
use crate::gfx::{Canvas, Rgb, big_width, ctext, ease_out, hash, hue, pal, prog, text, text_bg};
use crate::proto::{Kind, Link, Pid, Player, Session};

pub enum Bulbs {
    Chase(f32),
    Blaze,
}

/// Marquee bulbs around the border. Constant motion, so linear timing.
pub fn bulbs(buf: &mut Buffer, a: Rect, t: f32, mode: Bulbs) {
    let mut ring = vec![];
    for x in (a.left()..a.right()).step_by(2) {
        ring.push((x, a.top()));
    }
    for y in a.top() + 1..a.bottom() - 1 {
        ring.push((a.right() - 1, y));
    }
    for x in (a.left()..a.right()).rev().step_by(2) {
        ring.push((x, a.bottom() - 1));
    }
    for y in (a.top() + 1..a.bottom() - 1).rev() {
        ring.push((a.left(), y));
    }
    for (i, &(x, y)) in ring.iter().enumerate() {
        let c = match mode {
            Bulbs::Chase(speed) => {
                let on = (i + (t * speed) as usize) % 4 == 0 || (i + (t * speed) as usize) % 4 == 1;
                if on { pal::BULB } else { pal::BULB_OFF }
            }
            Bulbs::Blaze => if (i + (t * 30.0) as usize) % 2 == 0 { pal::INK } else { pal::BULB },
        };
        text(buf, a, x as i32, y as i32, "●", c);
    }
}

fn shimmer(lx: i32, ly: i32, h: i32, t: f32, w: i32) -> Rgb {
    let sweep = ((t * 0.45).fract() * (w + 40) as f32) as i32 - 20;
    let base = pal::BULB.lerp(pal::ORANGE, ly as f32 / h.max(1) as f32);
    if (lx + ly / 2 - sweep).abs() < 3 { base.lerp(pal::INK, 0.7) } else { base }
}

fn fit_scale(s: &str, max: i32, width: i32) -> i32 {
    let mut k = max;
    while k > 1 && big_width(s, k) > width - 6 {
        k -= 1;
    }
    k
}

fn link_badge(p: &Player) -> (String, Rgb) {
    match p.link {
        Link::Host => ("HOST".into(), pal::GOLD),
        Link::Bot => ("BOT".into(), pal::DIM),
        Link::Lan => (format!("LAN {}ms", p.ping), pal::GREEN),
        Link::Direct => (format!("NET {}ms", p.ping), pal::GREEN),
        Link::Relay => (format!("RELAY {}ms", p.ping), pal::ORANGE),
    }
}

fn short_code(code: &str) -> String {
    if code.len() > 12 { format!("{}…{}", &code[..6], &code[code.len() - 4..]) } else { code.to_string() }
}

/// Lobby: logo, room line, contestant podiums rising in, audience and controls.
pub fn lobby(buf: &mut Buffer, a: Rect, s: &Session, me: Pid, host: bool, t: f32, appear: &[(Pid, f32)]) {
    let mut cv = stage(a, t, 0.25);
    let scale = if a.width >= 118 && a.height >= 32 { 2 } else { 1 };
    let logo = "PARTYLINE";
    let lw = big_width(logo, scale);
    let lh = 7 * scale;
    cv.big_center(cv.w / 2, 4, logo, scale, &|x, y| shimmer(x, y, lh, t, lw));

    let players: Vec<&Player> = s.players.iter().filter(|p| !p.spectator).collect();
    let seats = players.len().max(4).min(8) as i32;
    let pw = ((a.width as i32 - 6) / seats - 2).min(16).max(9);
    let ph = ((cv.h as f32 * 0.34) as i32).clamp(16, 24);
    let base_y = (cv.h as f32 * 0.8) as i32;
    let x0 = (cv.w - seats * (pw + 2)) / 2 + 1;
    let mut labels = vec![];
    for i in 0..seats {
        let x = x0 + i * (pw + 2);
        let Some(p) = players.get(i as usize) else {
            // Open seat: dashed outline.
            for xx in x..x + pw {
                if xx % 2 == 0 {
                    cv.set(xx, base_y - ph, pal::DIM.mul(0.35));
                    cv.set(xx, base_y - 1, pal::DIM.mul(0.35));
                }
            }
            for yy in (base_y - ph..base_y).step_by(2) {
                cv.set(x, yy, pal::DIM.mul(0.35));
                cv.set(x + pw - 1, yy, pal::DIM.mul(0.35));
            }
            labels.push((x, (base_y - ph / 2) / 2, None));
            continue;
        };
        let t0 = appear.iter().find(|e| e.0 == p.id).map(|e| e.1).unwrap_or(0.0) + i as f32 * 0.06;
        let rise = ease_out(prog(t, t0, 0.36));
        let y = base_y - ph + ((1.0 - rise) * (ph + 8) as f32) as i32;
        let c = hue(p.hue);
        // Podium: hue cap, red face with vertical gradient, darker plate, wider base.
        for yy in 0..ph {
            let face = pal::PODIUM.lerp(pal::PODIUM_DARK, yy as f32 / ph as f32);
            cv.rect(x, y + yy, pw, 1, face);
        }
        cv.rect(x, y, pw, 3, c);
        cv.rect(x + 1, y + 6, pw - 2, 5, Rgb::hex(0x2A0A16));
        cv.rect(x - 1, y + ph - 2, pw + 2, 2, pal::PODIUM_DARK.mul(0.7));
        let bulb = if p.ready { pal::GREEN } else { pal::BULB_OFF };
        cv.rect(x + pw / 2 - 1, y + 12, 3, 2, bulb);
        if p.ready {
            cv.light((x + pw / 2) as f32, (y + 13) as f32, 6.0, pal::GREEN, 0.35);
        }
        if p.id == me {
            cv.light((x + pw / 2) as f32, (y - 2) as f32, pw as f32, c, 0.18);
        }
        labels.push((x, y / 2, Some(*p)));
    }
    cv.blit(buf, a);

    let room = format!("{}  ·  INVITE CODE {}  ·  C copies it", s.room.to_uppercase(), short_code(&s.code));
    let room_row = a.y as i32 + (4 + lh) / 2 + 2;
    ctext(buf, a, room_row, &room, pal::DIM);
    let lineup: Vec<&str> = crate::proto::KINDS.iter().map(|k| games::title(*k)).collect();
    ctext(buf, a, room_row + 2, &format!("TONIGHT  ·  {}", lineup.join("  ·  ")), pal::GOLD.mul(0.85));
    for (x, row, p) in labels {
        let ax = a.x as i32 + x;
        let ay = a.y as i32 + row;
        match p {
            None => text(buf, a, ax + (pw - 4) / 2, ay, "OPEN", pal::DIM.mul(0.6)),
            Some(p) => {
                let name: String = p.name.to_uppercase().chars().take(pw as usize - 2).collect();
                text(buf, a, ax + (pw - name.len() as i32) / 2, ay + 3, &name, pal::INK);
                let (badge, bc) = link_badge(p);
                let badge: String = badge.chars().take(pw as usize - 2).collect();
                text(buf, a, ax + (pw - badge.len() as i32) / 2, ay + 4, &badge, bc);
                let st = if p.ready { "READY" } else { "not ready" };
                text(buf, a, ax + (pw - st.len() as i32) / 2, ay + 7, st, if p.ready { pal::GREEN } else { pal::DIM });
                if p.id == me {
                    text(buf, a, ax + (pw - 5) / 2, ay - 1, "▼ YOU", hue(p.hue));
                }
            }
        }
    }

    let audience: Vec<String> = s.players.iter().filter(|p| p.spectator).map(|p| p.name.to_uppercase()).collect();
    let ready = players.iter().filter(|p| p.ready).count();
    let bottom = a.bottom() as i32 - 2;
    if !audience.is_empty() {
        ctext(buf, a, bottom - 2, &format!("AUDIENCE  {}", audience.join(" · ")), pal::DIM);
    }
    let status = format!("{ready}/{} READY   ·   {} ROUNDS", players.len(), s.rounds);
    ctext(buf, a, bottom - 1, &status, pal::GOLD);
    let keys = if host {
        "SPACE ready   S sit out   ←→ rounds   ENTER start the show   ESC leave"
    } else {
        "SPACE ready   S sit out   ESC leave   ·   the host starts the show"
    };
    ctext(buf, a, bottom, keys, pal::DIM);
    bulbs(buf, a, t, Bulbs::Chase(9.0));
}

/// Round title card, then a 3-2-1 countdown that breaks the grid.
pub fn intro(buf: &mut Buffer, a: Rect, s: &Session, kind: Kind, go_at: u64, now: u64, t: f32, since: f32) {
    let left = go_at.saturating_sub(now);
    let counting = left <= 3000;
    let mut cv = stage(a, t, if counting { 0.7 } else { 0.4 });
    let title = games::title(kind);
    let scale = fit_scale(title, 4, cv.w);
    let shown = ((t - since) / 0.035).max(0.0) as usize;
    let partial: String = title.chars().take(shown).collect();
    let tw = big_width(title, scale);
    let th = 7 * scale;
    let ty = if counting { 6 } else { cv.h / 2 - th };
    cv.big(cv.w / 2 - tw / 2 + scale, ty + scale, &partial, scale, &|_, _| pal::BLACK);
    cv.big(cv.w / 2 - tw / 2, ty, &partial, scale, &|x, y| shimmer(x, y, th, t, tw));
    if counting {
        let n = left.div_ceil(1000).max(1);
        let into = 1.0 - (left % 1000) as f32 / 1000.0;
        let digit = n.to_string();
        let max = ((cv.h - ty - th - 6) / 7).clamp(2, 9);
        let k = if into < 0.12 { max } else { (max - 1).max(2) };
        let c = pal::INK.lerp(pal::GOLD, into * 1.5);
        let cx = cv.w / 2;
        let cy = ty + th + 4 + ((max - k) * 7) / 2;
        cv.light(cx as f32, (cy + k * 3) as f32, (k * 8) as f32, pal::GOLD, 0.25 * (1.0 - into));
        cv.big_center(cx, cy, &digit, k, &|_, _| c);
    }
    cv.blit(buf, a);
    let (line, keys) = games::howto(kind);
    let row = a.y as i32 + (ty + th) / 2 + 2;
    ctext(buf, a, a.y as i32 + ty / 2 - 2, &format!("ROUND {} OF {}", s.round + 1, s.rounds), pal::GOLD);
    if !counting {
        ctext(buf, a, row, line, pal::INK);
        let pill = format!("  {keys}  ");
        let x = a.x as i32 + (a.width as i32 - pill.chars().count() as i32) / 2;
        text_bg(buf, a, x, row + 2, &pill, pal::STAGE_DEEP, pal::GOLD);
    } else {
        ctext(buf, a, a.bottom() as i32 - 3, line, pal::DIM);
    }
    bulbs(buf, a, t, if counting && (left % 1000) > 820 { Bulbs::Blaze } else { Bulbs::Chase(14.0) });
}

/// Per-round results: rows slide in, totals count up like a mechanical counter.
pub fn results(buf: &mut Buffer, a: Rect, s: &Session, kind: Kind, gains: &[(Pid, u32)], until: u64, now: u64, t: f32, since: f32) {
    let mut cv = stage(a, t, 0.35);
    let head = "RESULTS";
    let scale = fit_scale(head, 2, cv.w);
    cv.big_center(cv.w / 2, 4, head, scale, &|x, y| shimmer(x, y, 7 * scale, t, big_width(head, scale)));
    let top = 4 + 7 * scale + 6;
    let rw = (a.width as i32 - 10).min(60);
    let x0 = (cv.w - rw) / 2;
    let mut rows = vec![];
    let leader = s.players.iter().map(|p| p.score).max().unwrap_or(0);
    for (i, &(p, g)) in gains.iter().enumerate() {
        let d = since + 0.25 + i as f32 * 0.09;
        let k = ease_out(prog(t, d, 0.32));
        if k <= 0.0 {
            continue;
        }
        let slide = ((1.0 - k) * 10.0) as i32;
        let y = top + i as i32 * 6;
        let pl = s.player(p);
        let c = pl.map(|p| hue(p.hue)).unwrap_or(pal::DIM);
        cv.rect(x0 + slide, y, rw, 5, Rgb::hex(0x141B66).lerp(c, if i == 0 { 0.22 } else { 0.08 }));
        cv.rect(x0 + slide + 2, y + 1, 3, 3, c);
        let total = pl.map(|p| p.score).unwrap_or(0);
        let counted = total - g + (g as f32 * ease_out(prog(t, d + 0.35, 0.6))) as u32;
        rows.push((x0 + slide, y / 2, i, pl.map(|p| p.name.to_uppercase()).unwrap_or("GONE".into()), g, counted, total == leader && leader > 0));
    }
    cv.blit(buf, a);
    for (x, row, i, name, g, counted, lead) in rows {
        let ax = a.x as i32 + x;
        let ay = a.y as i32 + row;
        let place = ["1ST", "2ND", "3RD"].get(i).copied().map(String::from).unwrap_or(format!("{}TH", i + 1));
        text(buf, a, ax + 7, ay + 1, &place, if i == 0 { pal::GOLD } else { pal::DIM });
        text(buf, a, ax + 13, ay + 1, &name, pal::INK);
        if g > 0 {
            text(buf, a, ax + rw - 18, ay + 1, &format!("+{g}"), pal::GOLD);
        }
        text(buf, a, ax + rw - 10, ay + 1, &format!("{counted:>3} PTS"), pal::INK);
        if lead {
            text(buf, a, ax + rw - 2, ay + 1, "★", pal::GOLD);
        }
    }
    let leftf = (until.saturating_sub(now) as f32 / 5600.0).min(1.0);
    let next = if s.round + 1 < s.rounds { "NEXT ROUND" } else { "THE FINAL SCORES" };
    let bar = ((a.width as f32 - 20.0) * leftf) as usize;
    ctext(buf, a, a.bottom() as i32 - 4, &format!("{} · {}", games::title(kind), next), pal::DIM);
    ctext(buf, a, a.bottom() as i32 - 3, &"━".repeat(bar), pal::GOLD);
    bulbs(buf, a, t, Bulbs::Chase(10.0));
}

/// The champion's podium with confetti.
pub fn finale(buf: &mut Buffer, a: Rect, s: &Session, me: Pid, host: bool, t: f32, since: f32, l: &mut Local) {
    let mut cv = stage(a, t, 0.9);
    let mut ranked: Vec<&Player> = s.players.iter().filter(|p| !p.spectator).collect();
    ranked.sort_by_key(|p| std::cmp::Reverse(p.score));
    let Some(champ) = ranked.first() else {
        cv.blit(buf, a);
        return;
    };
    let cc = hue(champ.hue);
    let name = champ.name.to_uppercase();
    let floor = cv.h - 8;
    let name_y = 8;
    let max_scale = ((floor - 32 - 8 - name_y) / 7).clamp(1, 4);
    let scale = fit_scale(&name, max_scale, cv.w);
    let reveal = ease_out(prog(t, since + 1.1, 0.4));
    if reveal > 0.0 {
        cv.light((cv.w / 2) as f32, (name_y + 7 * scale / 2) as f32, cv.w as f32 * 0.5, cc, 0.3 * reveal);
        cv.big_center(cv.w / 2, name_y, &name, scale, &|_, y| cc.lerp(pal::INK, 0.35 - y as f32 * 0.02));
    }

    // Podium blocks: 2nd left, 1st center, 3rd right; 3rd rises first, champion last.
    let order = [(1usize, 0.55f32, 10i32), (0, 0.9, 16), (2, 0.25, 6)];
    let bw = (a.width as i32 / 6).clamp(12, 22);
    let mut names = vec![];
    for (slot, (rank, delay, height)) in order.into_iter().enumerate() {
        let Some(p) = ranked.get(rank) else { continue };
        let k = ease_out(prog(t, since + delay, 0.45));
        let h = (height as f32 * 2.0 * k) as i32;
        let x = cv.w / 2 + (slot as i32 - 1) * (bw + 2) - bw / 2;
        let c = hue(p.hue);
        for yy in 0..h {
            cv.rect(x, floor - h + yy, bw, 1, c.lerp(c.mul(0.45), yy as f32 / (height * 2) as f32));
        }
        cv.rect(x, floor - h, bw, 2, c.lerp(pal::INK, 0.4));
        let ns = if h >= 20 { 2 } else if h >= 11 { 1 } else { 0 };
        if k > 0.95 && ns > 0 {
            let num = (rank + 1).to_string();
            cv.big_center(x + bw / 2, floor - h + 3, &num, ns, &|_, _| pal::INK.lerp(c, 0.2));
        }
        names.push((x, (floor - h) / 2 - 2, p.name.to_uppercase(), p.score, k));
    }
    // Confetti: player hues and gold, falling forever.
    if l.parts.len() < 160 {
        let seed = (t * 60.0) as u32;
        for i in 0..3 {
            let h = hash(seed, i);
            let pick = ranked.get(h as usize % ranked.len()).map(|p| hue(p.hue)).unwrap_or(pal::GOLD);
            let c = if h % 3 == 0 { pal::GOLD } else { pick };
            l.parts.push(games::Part { x: (h % cv.w.max(1) as u32) as f32, y: -2.0, vx: ((h >> 8) % 20) as f32 - 10.0, vy: 8.0 + ((h >> 4) % 12) as f32, life: 4.0, c });
        }
    }
    let dt = (t - l.pt).clamp(0.0, 0.1);
    l.pt = t;
    l.draw_parts(&mut cv, dt, 12.0);
    cv.blit(buf, a);

    ctext(buf, a, a.y as i32 + 2, "TONIGHT'S CHAMPION", pal::GOLD);
    for (x, row, n, score, k) in names {
        if k > 0.95 {
            let label = format!("{n} {score}");
            text(buf, a, a.x as i32 + x + (bw - label.len() as i32) / 2, a.y as i32 + row, &label, pal::INK);
        }
    }
    let rest: Vec<String> = ranked.iter().skip(3).map(|p| format!("{} {}", p.name.to_uppercase(), p.score)).collect();
    if !rest.is_empty() {
        ctext(buf, a, a.bottom() as i32 - 3, &rest.join("   "), pal::DIM);
    }
    let keys = if host { "ENTER back to the lobby   ESC leave" } else { "the host will bring you back   ESC leave" };
    ctext(buf, a, a.bottom() as i32 - 2, keys, pal::DIM);
    let _ = me;
    bulbs(buf, a, t, Bulbs::Chase(22.0));
}

/// One-row header during play: round, game, live score chips.
pub fn hud(buf: &mut Buffer, a: Rect, s: &Session, kind: Kind) {
    let row = Rect::new(a.x, a.y, a.width, 1);
    for x in row.left()..row.right() {
        let c = &mut buf[(x, row.y)];
        c.set_char(' ');
        c.bg = pal::STAGE_DEEP.mul(0.7).color();
    }
    let left = format!(" ROUND {}/{} · {}", s.round + 1, s.rounds, games::title(kind));
    text(buf, row, row.x as i32, row.y as i32, &left, pal::GOLD);
    let mut ps: Vec<&Player> = s.players.iter().filter(|p| !p.spectator).collect();
    ps.sort_by_key(|p| std::cmp::Reverse(p.score));
    let mut x = row.right() as i32;
    for p in ps.iter().rev() {
        let chip = format!(" {} {} ", p.name.to_uppercase().chars().take(8).collect::<String>(), p.score);
        x -= chip.chars().count() as i32 + 1;
        if x < row.x as i32 + left.len() as i32 + 2 {
            break;
        }
        text_bg(buf, row, x, row.y as i32, &chip, pal::STAGE_DEEP, hue(p.hue));
    }
}

/// Broadcast lower third for spectators.
pub fn lower_third(buf: &mut Buffer, a: Rect, name: &str, c: Rgb, t: f32) {
    let y = a.bottom() as i32 - 3;
    let live = if (t * 1.6).fract() < 0.6 { pal::RED } else { pal::PODIUM_DARK };
    let label = format!(" CAM · {name} ");
    text_bg(buf, a, a.x as i32 + 2, y, " ● LIVE ", pal::INK, live.mul(0.8));
    text_bg(buf, a, a.x as i32 + 10, y, &label, pal::STAGE_DEEP, c);
    text(buf, a, a.x as i32 + 11 + label.len() as i32, y, "TAB switch camera", pal::DIM);
}

/// Giant GO! that breaks the grid for half a second.
pub fn go(buf: &mut Buffer, a: Rect, k: f32) {
    let mut cv = Canvas::for_area(a, pal::BLACK);
    for y in 0..cv.h {
        for x in 0..cv.w {
            let c = Rgb::from_color(buf[(a.x + x as u16, a.y + (y / 2) as u16)].bg);
            cv.set(x, y, c);
        }
    }
    let scale = fit_scale("GO!", 9, cv.w).min((cv.h / 9).max(2));
    let cy = cv.h / 2 - 7 * scale / 2;
    let cx = cv.w / 2;
    cv.big_center(cx, cy, "GO!", scale, &|_, y| pal::INK.lerp(pal::GOLD, y as f32 / (7 * scale) as f32 + (1.0 - k)));
    // Only stamp the lit glyph pixels so the game underneath stays visible.
    let mut mask = Canvas::for_area(a, pal::BLACK);
    mask.big(cx - big_width("GO!", scale) / 2, cy, "GO!", scale, &|_, _| pal::INK);
    for row in 0..a.height {
        for col in 0..a.width {
            let (x, y) = (col as i32, row as i32 * 2);
            let (top, bot) = (mask.get(x, y) != pal::BLACK, mask.get(x, y + 1) != pal::BLACK);
            if top || bot {
                let cell = &mut buf[(a.x + col, a.y + row)];
                let fg = if top { cv.get(x, y) } else { Rgb::from_color(cell.bg) };
                let bg = if bot { cv.get(x, y + 1) } else { Rgb::from_color(cell.bg) };
                cell.set_symbol("▀");
                cell.fg = fg.color();
                cell.bg = bg.color();
            }
        }
    }
}

/// CRT power transition. `p`: 0 = off (black), 1 = fully on.
/// Turning on opens a bright line vertically; turning off collapses to a line, then a dot.
pub fn crt(buf: &mut Buffer, a: Rect, p: f32, turning_on: bool) {
    if p >= 1.0 {
        return;
    }
    let (w, h) = (a.width as f32, a.height as f32);
    let (half_h, half_w) = if turning_on {
        (ease_out(p) * h / 2.0, w / 2.0)
    } else {
        let v = ((1.0 - p / 0.7).max(0.0)) * h / 2.0;
        let hw = if p < 0.7 { w / 2.0 } else { (1.0 - (p - 0.7) / 0.3).max(0.0) * w / 2.0 };
        (v.max(0.5), hw)
    };
    let glow = if turning_on { 1.0 - p } else { p };
    for row in 0..a.height {
        for col in 0..a.width {
            let (dx, dy) = ((col as f32 + 0.5 - w / 2.0).abs(), (row as f32 + 0.5 - h / 2.0).abs());
            let cell = &mut buf[(a.x + col, a.y + row)];
            if dy > half_h || dx > half_w {
                cell.set_char(' ');
                cell.bg = pal::BLACK.color();
                cell.fg = pal::BLACK.color();
            } else {
                let edge = (half_h - dy).min(half_w - dx) < 1.0;
                let k = if edge { 0.85 } else { glow * 0.6 };
                cell.fg = Rgb::from_color(cell.fg).lerp(pal::INK, k).color();
                cell.bg = Rgb::from_color(cell.bg).lerp(pal::INK, k).color();
            }
        }
    }
}
