//! Authoritative session (runs on the host): lobby, party flow, game ticks, bots.
//! Every connection, local or remote, talks to it through the same `Cmd` channel.
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot};

use crate::games::{Game, POINTS};
use crate::gfx::hash;
use crate::proto::{ClientMsg, Event, KINDS, Kind, Link, MAX_PLAYERS, Phase, Pid, Player, ServerMsg, Session};

pub enum Cmd {
    Join {
        name: String,
        password: String,
        link: Link,
        tx: mpsc::Sender<ServerMsg>,
        reply: oneshot::Sender<Result<Pid, String>>,
    },
    Msg(Pid, ClientMsg),
    Leave(Pid),
    Stats(Pid, Link, u16),
    Shutdown,
}

pub type Handle = mpsc::UnboundedSender<Cmd>;

const TICK: Duration = Duration::from_millis(16);
const INTRO_MS: u64 = 5200;
const RESULTS_MS: u64 = 5600;
const BOT_NAMES: [&str; 7] = ["ROBO", "BEEP", "ZAP", "BOLT", "CHIP", "DOT", "PIXEL"];

pub fn spawn(room: String, code: String, password: String, bots: u8) -> Handle {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut sv = Server {
        s: Session { room, code, players: vec![], phase: Phase::Lobby, round: 0, rounds: 4 },
        password,
        conns: vec![],
        bots: vec![],
        host: None,
        start: Instant::now(),
        ev: vec![],
        lineup: vec![],
        seed: fastrand::u64(..),
        frame: 0,
        last_count: 0,
        next_id: 1,
    };
    for name in BOT_NAMES.iter().take(bots as usize) {
        if let Some(id) = sv.add(name.to_string(), Link::Bot) {
            sv.bots.push(id);
        }
    }
    tokio::spawn(async move {
        let mut iv = tokio::time::interval(TICK);
        iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                cmd = rx.recv() => match cmd {
                    Some(Cmd::Shutdown) | None => break,
                    Some(c) => sv.cmd(c),
                },
                _ = iv.tick() => sv.tick(),
            }
        }
    });
    tx
}

struct Server {
    s: Session,
    password: String,
    conns: Vec<(Pid, mpsc::Sender<ServerMsg>)>,
    bots: Vec<Pid>,
    host: Option<Pid>,
    start: Instant,
    ev: Vec<Event>,
    lineup: Vec<Kind>,
    seed: u64,
    frame: u32,
    last_count: u8,
    next_id: Pid,
}

/// Trust boundary: names come from the network.
fn clean_name(raw: &str) -> String {
    let n: String = raw.trim().chars().filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '-').take(12).collect();
    let n = n.trim().to_string();
    if n.is_empty() { "PLAYER".into() } else { n }
}

impl Server {
    fn now(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }

    fn add(&mut self, name: String, link: Link) -> Option<Pid> {
        if self.s.players.len() >= MAX_PLAYERS {
            return None;
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let hue = (0..8u8).find(|h| self.s.players.iter().all(|p| p.hue != *h)).unwrap_or(0);
        let bot = link == Link::Bot;
        self.s.players.push(Player {
            id,
            name,
            hue,
            ready: bot,
            spectator: !matches!(self.s.phase, Phase::Lobby),
            score: 0,
            link,
            ping: 0,
        });
        self.ev.push(Event::Joined(id));
        Some(id)
    }

    fn remove(&mut self, p: Pid) {
        self.s.players.retain(|x| x.id != p);
        self.conns.retain(|c| c.0 != p);
    }

    fn participants(&self) -> Vec<Pid> {
        self.s.players.iter().filter(|p| !p.spectator).map(|p| p.id).collect()
    }

    fn cmd(&mut self, c: Cmd) {
        match c {
            Cmd::Join { name, password, link, tx, reply } => {
                if !self.password.is_empty() && password != self.password {
                    let _ = reply.send(Err("Wrong password.".into()));
                    return;
                }
                match self.add(clean_name(&name), link) {
                    Some(id) => {
                        if link == Link::Host && self.host.is_none() {
                            self.host = Some(id);
                        }
                        self.conns.push((id, tx));
                        let _ = reply.send(Ok(id));
                    }
                    None => {
                        let _ = reply.send(Err("Room is full.".into()));
                    }
                }
            }
            Cmd::Leave(p) => self.remove(p),
            Cmd::Stats(p, link, ping) => {
                if let Some(x) = self.s.players.iter_mut().find(|x| x.id == p) {
                    (x.link, x.ping) = (link, ping);
                }
            }
            Cmd::Msg(p, m) => self.msg(p, m),
            Cmd::Shutdown => {}
        }
    }

    fn msg(&mut self, p: Pid, m: ClientMsg) {
        let now = self.now();
        let is_host = self.host == Some(p);
        let lobby = matches!(self.s.phase, Phase::Lobby);
        match m {
            ClientMsg::Hello { .. } => {}
            ClientMsg::Ready(r) if lobby => {
                if let Some(x) = self.s.players.iter_mut().find(|x| x.id == p) {
                    x.ready = r;
                }
            }
            ClientMsg::Spectate(on) if lobby => {
                if let Some(x) = self.s.players.iter_mut().find(|x| x.id == p) {
                    x.spectator = on;
                }
            }
            ClientMsg::Rounds(n) if lobby && is_host => self.s.rounds = n.clamp(1, 12),
            ClientMsg::Start if lobby && is_host && !self.participants().is_empty() => {
                self.seed = fastrand::u64(..);
                self.lineup = (0..self.s.rounds as usize)
                    .map(|i| {
                        let mut k = KINDS;
                        let cycle = (i / KINDS.len()) as u32;
                        k.sort_by_key(|x| hash(self.seed as u32 ^ cycle, *x as u32));
                        k[i % KINDS.len()]
                    })
                    .collect();
                for x in &mut self.s.players {
                    x.score = 0;
                }
                self.s.round = 0;
                self.last_count = 0;
                self.s.phase = Phase::Intro { kind: self.lineup[0], go_at: now + INTRO_MS };
            }
            ClientMsg::Input(i) => {
                if let Phase::Play(g) = &mut self.s.phase {
                    g.input(p, &i, now, &mut self.ev);
                }
            }
            ClientMsg::ToLobby if is_host => {
                self.s.phase = Phase::Lobby;
                for x in &mut self.s.players {
                    x.ready = x.link == Link::Bot;
                }
            }
            _ => {}
        }
    }

    fn tick(&mut self) {
        let now = self.now();
        let phase = std::mem::replace(&mut self.s.phase, Phase::Lobby);
        self.s.phase = match phase {
            Phase::Intro { kind, go_at } => {
                let n = (go_at.saturating_sub(now).div_ceil(1000)) as u8;
                if (1..=3).contains(&n) && n != self.last_count {
                    self.last_count = n;
                    self.ev.push(Event::Count(n));
                }
                if now >= go_at {
                    self.last_count = 0;
                    self.ev.push(Event::Go);
                    let seed = hash(self.seed as u32, self.s.round as u32) as u64;
                    Phase::Play(Game::new(kind, &self.participants(), seed, now))
                } else {
                    Phase::Intro { kind, go_at }
                }
            }
            Phase::Play(mut g) => {
                for &b in &self.bots {
                    if let Some(i) = g.bot(b, now) {
                        g.input(b, &i, now, &mut self.ev);
                    }
                }
                match g.tick(now, &mut self.ev) {
                    Some(rank) => {
                        let gains: Vec<(Pid, u32)> =
                            rank.iter().enumerate().map(|(i, &p)| (p, POINTS.get(i).copied().unwrap_or(0))).collect();
                        for &(p, g) in &gains {
                            if let Some(x) = self.s.players.iter_mut().find(|x| x.id == p) {
                                x.score += g;
                            }
                        }
                        Phase::Results { kind: g.kind(), gains, until: now + RESULTS_MS }
                    }
                    None => Phase::Play(g),
                }
            }
            Phase::Results { kind, gains, until } => {
                if now < until {
                    Phase::Results { kind, gains, until }
                } else if self.s.round + 1 < self.s.rounds && (self.s.round as usize + 1) < self.lineup.len() {
                    self.s.round += 1;
                    Phase::Intro { kind: self.lineup[self.s.round as usize], go_at: now + INTRO_MS }
                } else {
                    Phase::Final
                }
            }
            other => other,
        };

        self.frame += 1;
        if self.frame % 2 == 0 || !self.ev.is_empty() {
            let msg = ServerMsg::Frame { now, s: Box::new(self.s.clone()), ev: std::mem::take(&mut self.ev) };
            let mut gone = vec![];
            for (p, tx) in &self.conns {
                if let Err(mpsc::error::TrySendError::Closed(_)) = tx.try_send(msg.clone()) {
                    gone.push(*p);
                }
            }
            for p in gone {
                self.remove(p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_sanitised() {
        assert_eq!(clean_name("  <script>alert(1)</script>  "), "scriptalert1");
        assert_eq!(clean_name("\u{1b}[31m"), "31m");
        assert_eq!(clean_name("   "), "PLAYER");
    }

    #[tokio::test]
    async fn a_party_with_bots_runs_to_the_final() {
        let h = spawn("test".into(), "code".into(), String::new(), 3);
        let (tx, mut rx) = mpsc::channel(512);
        let (reply, ok) = oneshot::channel();
        h.send(Cmd::Join { name: "host".into(), password: String::new(), link: Link::Host, tx, reply }).unwrap();
        let me = ok.await.unwrap().unwrap();
        h.send(Cmd::Msg(me, ClientMsg::Spectate(true))).unwrap();
        h.send(Cmd::Msg(me, ClientMsg::Rounds(1))).unwrap();
        h.send(Cmd::Msg(me, ClientMsg::Start)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            assert!(Instant::now() < deadline, "party never finished");
            if let Some(ServerMsg::Frame { s, .. }) = rx.recv().await {
                if matches!(s.phase, Phase::Final) {
                    assert!(s.players.iter().any(|p| p.score > 0));
                    break;
                }
            }
        }
    }
}
