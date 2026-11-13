//! Inside the TV: the test chamber. Contestants seated in the lobby, title cards, countdown,
//! results board, the champion's crate, and what sits over a game (score bar, camera, GO!).
//! Every word is pixels in one of the two faces in `gfx`.
use ratatui::{buffer::Buffer, layout::Rect, style::Style};

use crate::art::{self, EMPTY_CHAIR};
use crate::games::{self, Local, stage};
use crate::gfx::{Canvas, Hint, LINE, Rgb, SMALL, big_width, ease_out, fit_label, hash, hue, label_width, pal, pick_label, prog, type_h, wrap_label};
use crate::proto::{Kind, Link, Pid, Player, Session};

/// Where the feet touch the floor in a contestant sprite, as a fraction of its height.
const FEET: f32 = 0.9;
/// Rows the score bar takes at the top during play.
pub const HUD_ROWS: u16 = 4;
/// A chip or keycap: the small face on a plate.
const CHIP: i32 = SMALL + 2;

/// Stencil paint: bone white fading to hazard amber, with a slow highlight sweep.
fn shimmer(lx: i32, ly: i32, h: i32, t: f32, w: i32) -> Rgb {
    let sweep = ((t * 0.45).fract() * (w + 40) as f32) as i32 - 20;
    let base = pal::INK.lerp(pal::GOLD, ly as f32 / h.max(1) as f32);
    if (lx + ly / 2 - sweep).abs() < 3 { base.lerp(Rgb(1.0, 1.0, 1.0), 0.6) } else { base }
}

fn fit_scale(s: &str, max: i32, width: i32) -> i32 {
    let mut k = max;
    while k > 1 && big_width(s, k) > width - 6 {
        k -= 1;
    }
    k
}

/// How a player is connected, longest wording first.
fn link_badge(p: &Player) -> (Vec<String>, Rgb) {
    let ms = |k: &str, c| (vec![format!("{k} {}MS", p.ping), format!("{k} {}", p.ping), k.to_string()], c);
    match p.link {
        Link::Host => (vec!["HOST".into()], pal::GOLD),
        Link::Bot => (vec!["BOT".into()], pal::DIM),
        Link::Lan => ms("LAN", pal::GREEN),
        Link::Direct => ms("NET", pal::GREEN),
        Link::Relay => ms("RELAY", pal::ORANGE),
    }
}

/// Contestant sprite `i` (player hue, or `EMPTY_CHAIR`) standing on `feet` at center `cx`,
/// `h` pixels tall, with a contact shadow. Returns the sprite's top y.
fn seat(cv: &mut Canvas, i: usize, cx: f32, feet: f32, h: f32, k: f32) -> f32 {
    let art = art::get();
    let src = art.crew_src(i);
    let w = h * src.2 / src.3;
    let r = w * 0.32;
    for dx in -(r as i32)..=(r as i32) {
        let f = 1.0 - (dx as f32 / r).powi(2);
        for dy in -1..=1 {
            cv.blend(cx as i32 + dx, feet as i32 + dy, pal::BLACK, 0.55 * f * (1.0 - dy.abs() as f32 * 0.4));
        }
    }
    let top = feet - h * FEET;
    cv.img(&art.crew, src, (cx - w / 2.0, top, w, h), &|c| c.mul(k));
    top
}

/// Lobby: the room's name (under the logo when there is height for it), contestants dropping
/// into their chairs under their nameplates, who is ready, and the key hints, which carry the
/// counts.
pub fn lobby(buf: &mut Buffer, a: Rect, s: &Session, me: Pid, host: bool, t: f32, appear: &[(Pid, f32)]) {
    let mut cv = stage(a, t, 0.45);
    let (w, h, cx) = (cv.w, cv.h, cv.w / 2);
    let players: Vec<&Player> = s.players.iter().filter(|p| !p.spectator).collect();
    let seats = players.len().clamp(4, 8);
    let slot = ((w - 6) as f32 / seats as f32).min(40.0);
    let x0 = w as f32 / 2.0 - slot * (seats as f32 - 1.0) / 2.0;

    let mut y = 3;
    if h >= 80 {
        let scale = if w >= 118 && h >= 96 { 2 } else { 1 };
        let (lw, lh) = (big_width("PARTYLINE", scale), 7 * scale);
        cv.big_center(cx, y, "PARTYLINE", scale, &|x, yy| shimmer(x, yy, lh, t, lw));
        y += type_h(lh as f32) + 3;
    }
    cv.heading(cx, y, &s.room.to_uppercase(), pal::GOLD, w - 8);
    y += SMALL + 4;
    // The invite code gets the next whole cell row (filled in after the blit).
    let code_row = (y - 1) / 2;
    y = code_row * 2 + 6;

    // Nameplates wider than a seat take turns high and low.
    let names: Vec<String> = players.iter().map(|p| p.name.to_uppercase()).collect();
    let crowded = names.iter().any(|n| label_width(n, 1) + 4 > slot as i32 - 2);
    let plate_max = if crowded { (2.0 * slot) as i32 - 4 } else { slot as i32 - 2 };
    let plates = if crowded { 2 * (CHIP + 1) } else { CHIP + 1 };

    // From the bottom: key hints, connection badges when there is room, then the floor. The
    // READY chip sits on each contestant's feet.
    let keys_y = h - 2 - CHIP;
    let badges = h >= 72 && slot >= 30.0;
    let badge_y = keys_y - 4 - SMALL;
    let feet = (if badges { badge_y - 3 } else { keys_y - 4 }) as f32;
    let chip_y = feet as i32 - CHIP + 1;
    let top = (y + plates) as f32;
    let sh = ((feet - top) / FEET).min(slot * 2.3).max(10.0);

    let mut tags = vec![];
    for i in 0..seats {
        let scx = x0 + i as f32 * slot;
        let Some(p) = players.get(i) else {
            let top = seat(&mut cv, EMPTY_CHAIR, scx, feet, sh, 0.55);
            tags.push((scx, top, i, None));
            continue;
        };
        let t0 = appear.iter().find(|e| e.0 == p.id).map(|e| e.1).unwrap_or(0.0) + i as f32 * 0.06;
        let drop = (1.0 - ease_out(prog(t, t0, 0.4))) * feet;
        if p.ready {
            cv.light(scx, feet, slot * 0.6, pal::GREEN, 0.22);
        }
        if p.id == me {
            cv.light(scx, feet - sh * 0.75, slot * 0.7, pal::BULB, 0.16);
        }
        let top = seat(&mut cv, p.hue as usize % 8, scx, feet - drop, sh, 1.25);
        tags.push((scx, top, i, Some(*p)));
    }
    // Floor shadow under the chips and key hints.
    cv.band(feet as i32 + 1, h, pal::BLACK, 0.65);

    for (scx, top, i, p) in tags {
        let sx = scx as i32;
        let lift = if crowded && i % 2 == 1 { CHIP + 1 } else { 0 };
        let py = top as i32 - CHIP - 1 - lift;
        let Some(p) = p else {
            cv.label_center(sx, py + 2, "OPEN", pal::DIM, 1);
            continue;
        };
        // Your nameplate is lit in your color; everyone else's name is in theirs.
        let name = fit_label(&p.name.to_uppercase(), plate_max - 4, 1);
        let (fg, bg) = if p.id == me { (pal::STAGE_DEEP, hue(p.hue)) } else { (hue(p.hue), pal::PANEL) };
        cv.chip_center(sx, py, &name, fg, bg);
        if p.ready {
            cv.chip_center(sx, chip_y, if slot >= 25.0 { "READY" } else { "✓" }, pal::STAGE_DEEP, pal::GREEN);
        }
        if badges {
            let (b, c) = link_badge(p);
            cv.label_center(sx, badge_y, &pick_label(&b, slot as i32 - 2), c, 1);
        }
    }

    let ready = format!("ready {}/{}", players.iter().filter(|p| p.ready).count(), players.len());
    let rounds = format!("{} rounds", s.rounds);
    let guest = format!("{} rounds · the host starts", s.rounds);
    let watching: Vec<String> = s.players.iter().filter(|p| p.spectator).map(|p| p.name.to_uppercase()).collect();
    let watch = match watching.len() {
        0 => String::new(),
        1 => format!("{} is watching", watching[0]),
        n => format!("{n} watching"),
    };
    let mut hints: Vec<Hint> = if host {
        vec![("ENTER", "start"), ("SPACE", ready.as_str()), ("←→", rounds.as_str()), ("C", "copy invite"), ("S", "sit out"), ("ESC", "leave")]
    } else {
        vec![("SPACE", ready.as_str()), ("", guest.as_str()), ("C", "copy invite"), ("S", "sit out"), ("ESC", "leave")]
    };
    if !watch.is_empty() {
        hints.push(("", watch.as_str()));
    }
    cv.keys(cx, keys_y, w - 8, &hints, t);
    cv.blit(buf, a);
    invite(buf, a, code_row as u16, &s.code);
}

/// The invite code on cell row `row`, as terminal text: 64 characters don't fit in pixel type,
/// and real text can be selected with the mouse when the clipboard is out of reach.
fn invite(buf: &mut Buffer, a: Rect, row: u16, code: &str) {
    const TAG: &str = " INVITE ";
    let n = code.chars().count() as u16 + 1;
    let tag = if n + TAG.len() as u16 + 4 <= a.width { TAG } else { " " };
    let x = a.x + a.width.saturating_sub(n + tag.len() as u16) / 2;
    let plate = Style::new().bg(pal::PANEL.color());
    buf.set_string(x, a.y + row, tag, plate.fg(pal::DIM.color()));
    buf.set_stringn(x + tag.len() as u16, a.y + row, format!("{code} "), a.width as usize, plate.fg(pal::GOLD.color()));
}

/// Round title card, then a 3-2-1 countdown that breaks the grid.
pub fn intro(buf: &mut Buffer, a: Rect, s: &Session, kind: Kind, go_at: u64, now: u64, t: f32, since: f32) {
    let left = go_at.saturating_sub(now);
    let counting = left <= 3000;
    let mut cv = stage(a, t, if counting { 0.6 } else { 0.35 });
    let cx = cv.w / 2;
    let title = games::title(kind);
    let (line, hints) = games::howto(kind);
    let mut lines = wrap_label(line, (cv.w - 8).min(180));
    // The card, top down: the round, the title, then the rules and keys (or, counting down,
    // the digit, at least 14 px, and the rules). Until it fits, the title shrinks, then the
    // keys go, then the round (the score bar shows it in play), then the rules.
    let rules = |n: usize| n as i32 * LINE;
    let below = |n: usize, keys: bool| if counting { 4 + 16 + 4 + rules(n) } else { 5 + rules(n) + if keys { 4 + CHIP } else { 0 } };
    let block = |scale: i32, n: usize, keys: bool, round: bool| if round { LINE + 2 } else { 0 } + type_h((7 * scale) as f32) + below(n, keys);
    let fits = |scale, n, keys, round| block(scale, n, keys, round) + 3 <= cv.h;
    let (mut keys, mut round) = (true, true);
    let mut scale = fit_scale(title, 4, cv.w);
    while scale > 1 && !fits(scale, lines.len(), keys, round) {
        scale -= 1;
    }
    keys = fits(scale, lines.len(), keys, round);
    round = fits(scale, lines.len(), keys, round);
    if !fits(scale, lines.len(), keys, round) {
        lines.clear();
        round = fits(scale, 0, keys, true);
    }
    let shown = ((t - since) / 0.035).max(0.0) as usize;
    let partial: String = title.chars().take(shown).collect();
    let tw = big_width(title, scale);
    let th = 7 * scale;
    let lead = if round { LINE + 2 } else { 0 };
    let ty = if counting { 2 + lead } else { ((cv.h - block(scale, lines.len(), keys, round)) / 2).max(2) + lead };
    if round {
        cv.heading(cx, ty - LINE - 2, &format!("ROUND {} OF {}", s.round + 1, s.rounds), pal::GOLD, cv.w - 8);
    }
    cv.big(cx - tw / 2 + scale, ty + scale, &partial, scale, &|_, _| pal::BLACK);
    cv.big(cx - tw / 2, ty, &partial, scale, &|x, y| shimmer(x, y, th, t, tw));
    if counting {
        let foot = cv.h - 2 - rules(lines.len());
        let n = left.div_ceil(1000).max(1);
        let into = 1.0 - (left % 1000) as f32 / 1000.0;
        let max = ((foot - ty - th - 10) / 7).clamp(2, 9);
        let k = if into < 0.12 { max } else { (max - 1).max(2) };
        let c = pal::INK.lerp(pal::RED, into * 1.3);
        let cy = ty + th + 4 + ((max - k) * 7) / 2;
        cv.light(cx as f32, (cy + k * 3) as f32, (k * 8) as f32, pal::BULB, 0.22 * (1.0 - into));
        cv.big_center(cx, cy, &n.to_string(), k, &|_, _| c);
        for (i, l) in lines.iter().enumerate() {
            cv.label_center(cx, foot + i as i32 * LINE, l, pal::DIM, 1);
        }
    } else {
        let mut y = ty + type_h(th as f32) + 5;
        for l in &lines {
            cv.label_center(cx, y, l, pal::INK, 1);
            y += LINE;
        }
        if keys {
            cv.keys(cx, y + 4, cv.w - 8, hints, t);
        }
    }
    cv.blit(buf, a);
}

/// Per-round results: rows slide in, totals count up like a mechanical counter. A crowd
/// splits into two columns.
pub fn results(buf: &mut Buffer, a: Rect, s: &Session, gains: &[(Pid, u32)], until: u64, now: u64, t: f32, since: f32) {
    let mut cv = stage(a, t, 0.3);
    let (w, h, cx) = (cv.w, cv.h, cv.w / 2);
    let head = "RESULTS";
    let scale = if h >= 70 { fit_scale(head, 2, w) } else { 1 };
    cv.big_center(cx, 2, head, scale, &|x, y| shimmer(x, y, 7 * scale, t, big_width(head, scale)));
    let top = 2 + type_h((7 * scale) as f32) + 3;
    // From the bottom: the clock bar, then "up next" when the rows leave room for it. One
    // column if the rows fit (with the note if they can), else two.
    let bar_y = h - 3;
    let note_y = bar_y - 2 - LINE;
    let n = gains.len() as i32;
    let rows_fit = |cols: i32, foot: i32| (n + cols - 1) / cols * 7 <= foot - top;
    let (cols, note) = [(1, true), (1, false), (2, true), (2, false)].into_iter().find(|&(c, nt)| rows_fit(c, if nt { note_y - 2 } else { bar_y - 2 })).unwrap_or((2, false));
    let foot = if note { note_y - 2 } else { bar_y - 2 };
    let per = (n + cols - 1) / cols;
    let pitch = ((foot - top) / per.max(1)).clamp(7, 10);
    let cw = if cols == 1 { (w - 12).min(110) } else { ((w - 14) / 2).min(80) };
    let gx = (w - (cw * cols + 4 * (cols - 1))) / 2;
    let leader = s.players.iter().map(|p| p.score).max().unwrap_or(0);
    for (i, &(p, g)) in gains.iter().enumerate() {
        let d = since + 0.25 + i as f32 * 0.09;
        let k = ease_out(prog(t, d, 0.32));
        if k <= 0.0 {
            continue;
        }
        let (col, row) = (i as i32 / per, i as i32 % per);
        let x = gx + col * (cw + 4) + ((1.0 - k) * 10.0) as i32;
        let y = top + row * pitch;
        let rh = pitch - 1;
        let pl = s.player(p);
        let c = pl.map(|p| hue(p.hue)).unwrap_or(pal::DIM);
        cv.rect(x, y, cw, rh, pal::PANEL.lerp(c, if i == 0 { 0.25 } else { 0.08 }));
        cv.rect(x, y, 2, rh, c);
        cv.rect(x, y + rh - 1, cw, 1, pal::BLACK.lerp(pal::PANEL, 0.3));
        let ty = y + (rh - 1 - SMALL) / 2;
        let place = match (cols, i) {
            (2, _) => (i + 1).to_string(),
            (_, 0) => "1ST".to_string(),
            (_, 1) => "2ND".into(),
            (_, 2) => "3RD".into(),
            _ => format!("{}TH", i + 1),
        };
        cv.label(x + 5, ty, &place, if i == 0 { pal::GOLD } else { pal::DIM }, 1);
        // Right to left: the leader's star, the running total (tabular digits), the gain.
        let total = pl.map(|p| p.score).unwrap_or(0);
        let counted = total - g + (g as f32 * ease_out(prog(t, d + 0.35, 0.6))) as u32;
        let mut rx = x + cw - 4;
        if total == leader && leader > 0 {
            rx -= 5;
            cv.label(rx, ty, "★", pal::GOLD, 1);
            rx -= 4;
        }
        // A narrow row keeps the name over the gain and the "PTS".
        let pts = if cw >= 90 { format!("{counted} PTS") } else { counted.to_string() };
        rx -= label_width(&pts, 1);
        cv.label(rx, ty, &pts, pal::INK, 1);
        if g > 0 && cw >= 80 {
            let gain = format!("+{g}");
            rx -= label_width(&gain, 1) + 5;
            cv.label(rx, ty, &gain, pal::GOLD, 1);
        }
        let nx = x + 5 + label_width(if cols == 2 { "8" } else { "8TH" }, 1) + 5;
        let name = pl.map(|p| p.name.to_uppercase()).unwrap_or("GONE".into());
        cv.label(nx, ty, &fit_label(&name, rx - 5 - nx, 1), pal::INK, 1);
    }
    let leftf = (until.saturating_sub(now) as f32 / 5600.0).min(1.0);
    if note {
        let next = if s.round + 1 < s.rounds { format!("UP NEXT · ROUND {}", s.round + 2) } else { "UP NEXT · THE FINAL SCORES".into() };
        cv.heading(cx, note_y, &next, pal::DIM, w - 8);
    }
    let bw = ((w - 20) as f32 * leftf) as i32;
    cv.rect(cx - bw / 2, bar_y, bw, 1, pal::GOLD);
    cv.blit(buf, a);
}

/// The champion on the tallest crate, with confetti.
pub fn finale(buf: &mut Buffer, a: Rect, s: &Session, host: bool, t: f32, since: f32, l: &mut Local) {
    let mut cv = stage(a, t, 0.8);
    let mut ranked: Vec<&Player> = s.players.iter().filter(|p| !p.spectator).collect();
    ranked.sort_by_key(|p| std::cmp::Reverse(p.score));
    let Some(champ) = ranked.first() else {
        cv.blit(buf, a);
        return;
    };
    let (w, h, cx) = (cv.w, cv.h, cv.w / 2);
    let cc = hue(champ.hue);
    // From the bottom: key hints, everyone's score on chips when there is room, the floor.
    let keys_y = h - 2 - CHIP;
    let board = h >= 70;
    let board_y = keys_y - 3 - CHIP;
    let floor = if board { board_y - 4 } else { keys_y - 4 };
    let name = champ.name.to_uppercase();
    cv.heading(cx, 2, "TONIGHT'S CHAMPION", pal::GOLD, w - 8);
    let name_y = 2 + LINE + 3;
    // The name is as big as it can be while the crates and contestants still stand under it.
    let tall = (h as f32 * 0.12) as i32;
    let stand = |scale: i32| floor - tall - (name_y + type_h((7 * scale) as f32) + 3);
    let mut scale = fit_scale(&name, 4, w);
    while scale > 1 && (stand(scale) as f32) < h as f32 * 0.3 {
        scale -= 1;
    }
    let sprite_h = (stand(scale) as f32 / FEET).min(h as f32 * 0.4).max(10.0);
    let reveal = ease_out(prog(t, since + 1.3, 0.4));
    if reveal > 0.0 {
        cv.light(cx as f32, (name_y + 7 * scale / 2) as f32, w as f32 * 0.5, cc, 0.3 * reveal);
        cv.big_center(cx, name_y, &name, scale, &|_, y| cc.lerp(pal::INK, 0.4 - y as f32 * 0.02));
    }

    // Crates: 2nd left, 1st center, 3rd right; 3rd rises first, the champion last.
    let order = [(1usize, 0.55f32, 0.62f32), (0, 0.9, 1.0), (2, 0.25, 0.38)];
    let bw = (w / 6).clamp(12, 24);
    for (slot, (rank, delay, height)) in order.into_iter().enumerate() {
        let Some(p) = ranked.get(rank) else { continue };
        let k = ease_out(prog(t, since + delay, 0.45));
        let ch = (tall as f32 * height * k) as i32;
        let x = cx + (slot as i32 - 1) * (bw + 3) - bw / 2;
        for yy in 0..ch {
            let plank = (ch - yy) % 5 == 0;
            cv.rect(x, floor - ch + yy, bw, 1, if plank { pal::WOOD.mul(0.55) } else { pal::WOOD.mul(0.9 - yy as f32 / (tall * 3) as f32) });
        }
        cv.rect(x, floor - ch, bw, 1, hue(p.hue));
        if k > 0.95 && ch >= 9 {
            cv.big_center(x + bw / 2, floor - ch + 3, &(rank + 1).to_string(), 1, &|_, _| pal::INK.mul(0.85));
        }
        if k > 0.9 {
            let drop = (1.0 - ease_out(prog(t, since + delay + 0.45, 0.3))) * h as f32;
            seat(&mut cv, p.hue as usize % 8, (x + bw / 2) as f32, (floor - ch) as f32 - drop, sprite_h, 1.25);
        }
    }
    // Confetti: player hues and amber, falling forever.
    if l.parts.len() < 160 {
        let seed = (t * 60.0) as u32;
        for i in 0..3 {
            let hh = hash(seed, i);
            let pick = ranked.get(hh as usize % ranked.len()).map(|p| hue(p.hue)).unwrap_or(pal::GOLD);
            let c = if hh % 3 == 0 { pal::GOLD } else { pick };
            l.parts.push(games::Part { x: (hh % w.max(1) as u32) as f32, y: -2.0, vx: ((hh >> 8) % 20) as f32 - 10.0, vy: 8.0 + ((hh >> 4) % 12) as f32, life: 4.0, c });
        }
    }
    let dt = (t - l.pt).clamp(0.0, 0.1);
    l.pt = t;
    l.draw_parts(&mut cv, dt, 12.0);
    cv.band(floor + 1, h, pal::BLACK, 0.6);

    // The final board, best first, as many as fit.
    if board {
        let chips: Vec<(String, Rgb)> = ranked.iter().map(|p| (format!("{} {}", fit_label(&p.name.to_uppercase(), 32, 1), p.score), hue(p.hue))).collect();
        let mut n = 0;
        let mut total = -3;
        for (s, _) in &chips {
            let cw = label_width(s, 1) + 4 + 3;
            if total + cw > w - 8 {
                break;
            }
            total += cw;
            n += 1;
        }
        let mut x = cx - total / 2;
        for (s, c) in &chips[..n] {
            x += cv.chip(x, board_y, s, pal::STAGE_DEEP, *c) + 3;
        }
    }
    let hints: &[Hint] = if host { &[("ENTER", "back to the lobby"), ("ESC", "leave")] } else { &[("", "the host brings everyone back"), ("ESC", "leave")] };
    cv.keys(cx, keys_y, w - 8, hints, t);
    cv.blit(buf, a);
}

/// The score bar during play: the game and round on the left, the game's own count on the
/// right, and the leaders' scores between them as far as they fit.
pub fn hud(buf: &mut Buffer, a: Rect, s: &Session, kind: Kind, status: (String, Rgb)) {
    let bar = Rect::new(a.x, a.y, a.width, HUD_ROWS.min(a.height));
    let mut cv = Canvas::for_area(bar, pal::PANEL);
    cv.rect(0, cv.h - 1, cv.w, 1, pal::BLACK.lerp(pal::PANEL, 0.4));
    let y = (cv.h - 1 - SMALL) / 2;
    let right = cv.w - 4 - label_width(&status.0, 1);
    cv.label(right, y, &status.0, status.1, 1);
    // Left: the game, then the round, each when it doesn't run into the status.
    let mut x = 4;
    let title = games::title(kind);
    if x + label_width(title, 1) + 8 <= right {
        x += cv.label(x, y, title, pal::GOLD, 1) + 6;
    }
    let round = format!("ROUND {}/{}", s.round + 1, s.rounds);
    if x + label_width(&round, 1) + 8 <= right {
        x += cv.label(x, y, &round, pal::DIM, 1) + 10;
    }
    let mut ps: Vec<&Player> = s.players.iter().filter(|p| !p.spectator).collect();
    ps.sort_by_key(|p| std::cmp::Reverse(p.score));
    for p in ps {
        let chip = format!("{} {}", fit_label(&p.name.to_uppercase(), 24, 1), p.score);
        if x + label_width(&chip, 1) + 4 > right - 8 {
            break;
        }
        x += cv.chip(x, y - 1, &chip, pal::STAGE_DEEP, hue(p.hue)) + 3;
    }
    cv.blit(buf, bar);
}

/// Broadcast lower third for spectators.
pub fn lower_third(buf: &mut Buffer, a: Rect, name: &str, c: Rgb, t: f32) {
    let mut cv = Canvas::from_buf(buf, a);
    let y = cv.h - 4 - CHIP;
    let live = if (t * 1.6).fract() < 0.6 { pal::RED } else { pal::PODIUM_DARK };
    let mut x = 6;
    x += cv.chip(x, y, "● REC", pal::INK, live.mul(0.8)) + 2;
    x += cv.chip(x, y, &format!("CAM · {name}"), pal::STAGE_DEEP, c) + 8;
    x += cv.chip(x, y, "TAB", pal::STAGE_DEEP, pal::INK.mul(0.78)) + 3;
    cv.label(x, y + 1, "switch camera", pal::DIM, 1);
    cv.blit(buf, a);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lobby_shows_whole_invite_code_at_minimum_size() {
        let code = "0123456789abcdef".repeat(4);
        let s = Session { room: "r".into(), code: code.clone(), players: vec![], phase: crate::proto::Phase::Lobby, round: 0, rounds: 4 };
        let a = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(a);
        lobby(&mut buf, a, &s, 1, true, 10.0, &[]);
        crate::gfx::crt_glass(&mut buf, a, 10.0);
        let rows: Vec<String> = (0..a.height).map(|y| (0..a.width).map(|x| buf[(x, y)].symbol()).collect()).collect();
        assert!(rows.iter().any(|r| r.contains(&format!("INVITE {code}"))));
    }
}
