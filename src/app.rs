//! The app: living-room TV menu, camera dolly into the screen, the studio session, input.
use std::io::Write as _;
use std::time::{Duration, Instant};

use iroh::EndpointId;
use ratatui::crossterm::event::{self, Event as Ct, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, Frame, buffer::Buffer, layout::Rect};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::games::{Fx, Game, Local, View, derby};
use crate::gfx::{self, Rgb, big_width, ctext, ease_in_out, hash, hue, pal, prog, text, text_bg};
use crate::net::{self, Conn, Lan, Net, RoomAd};
use crate::proto::{ClientMsg, Dir, Event, Input, Kind, Phase, Pid, ServerMsg, Session};
use crate::room;
use crate::server::{self, Cmd};
use crate::sfx::{Sfx, Sound};
use crate::studio;

pub struct Args {
    pub name: String,
    pub bots: u8,
    pub calm: bool,
    pub mute: bool,
}

const ZOOM_IN: f32 = 0.7;
const ZOOM_OUT: f32 = 0.55;
const POWER_ON: f32 = 0.26;
const POWER_OFF: f32 = 0.32;
/// The idle CRT's cold blue-white light.
const TV_COLD: Rgb = Rgb::hex(0xA9C4D6);

#[derive(Clone, PartialEq)]
enum Page {
    Main,
    Host,
    Join,
    Code,
    Password,
    Connecting(String),
    Error(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Name,
    Room,
    Password,
    Code,
    JoinPw,
}

#[derive(Clone, Copy, PartialEq)]
enum Item {
    Host,
    Join,
    Name,
    Quit,
    RoomName,
    Password,
    Bots,
    Open,
    BackMain,
    BackJoin,
    Room(usize),
    Code,
    CodeGo,
    JoinPw,
    PwGo,
    Ok,
}

#[derive(Clone, Copy)]
enum Mode {
    Room,
    ZoomIn(f32),
    Studio(f32),
    PowerOff(f32),
    ZoomOut(f32),
}

struct Hosting {
    accept: Option<JoinHandle<()>>,
    server: server::Handle,
}

type Pending = oneshot::Receiver<anyhow::Result<(Conn, Option<Hosting>)>>;

pub struct App {
    rt: tokio::runtime::Handle,
    name: String,
    calm: bool,
    sfx: Sfx,
    t0: Instant,
    last_t: f32,
    quit: bool,
    area: Rect,
    // TV menu
    page: Page,
    sel: usize,
    edit: Option<(Field, String)>,
    chan_t: f32,
    room: String,
    password: String,
    bots: u8,
    code: String,
    join_pw: String,
    target: Option<EndpointId>,
    lan: Vec<RoomAd>,
    lan_rx: Option<mpsc::UnboundedReceiver<Lan>>,
    net: Option<Net>,
    net_rx: Option<oneshot::Receiver<anyhow::Result<Net>>>,
    net_err: Option<String>,
    glow: Rgb,
    mode: Mode,
    // Session
    pending: Option<Pending>,
    conn: Option<Conn>,
    hosting: Option<Hosting>,
    session: Option<Session>,
    frame_at: Instant,
    server_now: u64,
    local: Local,
    fx: Fx,
    cam: Pid,
    game_id: Option<(u8, Kind)>,
    appear: Vec<(Pid, f32)>,
    phase_tag: u8,
    phase_since: f32,
    go_t: f32,
    studio_t: f32,
    toast: Option<(String, f32)>,
}

impl App {
    pub fn new(rt: tokio::runtime::Handle, args: Args) -> App {
        let (tx, net_rx) = oneshot::channel();
        rt.spawn(async move {
            let _ = tx.send(net::start().await);
        });
        App {
            rt,
            room: format!("{}'s room", args.name.to_lowercase()),
            name: args.name,
            calm: args.calm,
            sfx: Sfx::new(!args.mute),
            t0: Instant::now(),
            last_t: 0.0,
            quit: false,
            area: Rect::default(),
            page: Page::Main,
            sel: 0,
            edit: None,
            chan_t: -1.0,
            password: String::new(),
            bots: args.bots,
            code: String::new(),
            join_pw: String::new(),
            target: None,
            lan: vec![],
            lan_rx: None,
            net: None,
            net_rx: Some(net_rx),
            net_err: None,
            glow: TV_COLD,
            mode: Mode::Room,
            pending: None,
            conn: None,
            hosting: None,
            session: None,
            frame_at: Instant::now(),
            server_now: 0,
            local: Local::default(),
            fx: Fx::default(),
            cam: 0,
            game_id: None,
            appear: vec![],
            phase_tag: 255,
            phase_since: 0.0,
            go_t: -10.0,
            studio_t: 0.0,
            toast: None,
        }
    }

    fn t(&self) -> f32 {
        self.t0.elapsed().as_secs_f32()
    }

    pub fn run(&mut self, term: &mut DefaultTerminal) -> anyhow::Result<()> {
        let frame = Duration::from_millis(16);
        while !self.quit {
            let start = Instant::now();
            while event::poll(Duration::ZERO)? {
                let e = event::read()?;
                self.on_input(e);
            }
            self.update();
            term.draw(|f| self.draw(f))?;
            if let Some(wait) = frame.checked_sub(start.elapsed()) {
                event::poll(wait)?;
            }
        }
        self.shutdown_hosting();
        Ok(())
    }

    // ---------------- update ----------------

    fn update(&mut self) {
        let t = self.t();
        let dt = (t - self.last_t).clamp(0.0, 0.1);
        self.last_t = t;

        if let Some(rx) = &mut self.net_rx {
            match rx.try_recv() {
                Ok(Ok(net)) => {
                    self.lan_rx = Some(net.scan());
                    self.net = Some(net);
                    self.net_rx = None;
                }
                Ok(Err(e)) => {
                    self.net_err = Some(e.to_string());
                    self.net_rx = None;
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(_) => self.net_rx = None,
            }
        }
        if let Some(rx) = &mut self.lan_rx {
            while let Ok(e) = rx.try_recv() {
                match e {
                    Lan::Up(ad) => {
                        self.lan.retain(|r| r.id != ad.id);
                        self.lan.push(ad);
                    }
                    Lan::Down(id) => self.lan.retain(|r| r.id != id),
                }
            }
        }
        if let Some(rx) = &mut self.pending {
            match rx.try_recv() {
                Ok(Ok((conn, hosting))) => {
                    self.pending = None;
                    self.cam = conn.me;
                    self.conn = Some(conn);
                    self.hosting = hosting;
                    self.session = None;
                    self.appear.clear();
                    self.phase_tag = 255;
                    self.page = Page::Main;
                    self.mode = if self.calm { Mode::Studio(t - POWER_ON) } else { Mode::ZoomIn(t) };
                    self.sfx.play(Sound::Whoosh);
                }
                Ok(Err(e)) => {
                    self.pending = None;
                    self.shutdown_hosting();
                    self.goto(Page::Error(friendly(&e)));
                }
                Err(oneshot::error::TryRecvError::Empty) => {}
                Err(_) => self.pending = None,
            }
        }

        // Session frames.
        let mut frames = vec![];
        let mut lost = false;
        if let Some(c) = &mut self.conn {
            loop {
                match c.rx.try_recv() {
                    Ok(ServerMsg::Frame { now, s, ev }) => frames.push((now, *s, ev)),
                    Ok(_) => {}
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        lost = true;
                        break;
                    }
                }
            }
        }
        for (now, s, ev) in frames {
            self.on_frame(now, s, ev);
        }
        if lost {
            self.leave(Some("The show went off the air. The host left or the connection dropped.".into()));
        }

        // Mode transitions.
        if let Mode::Studio(s) = self.mode {
            self.studio_t = s;
        }
        self.mode = match self.mode {
            Mode::ZoomIn(s) if t - s >= ZOOM_IN => Mode::Studio(t),
            Mode::PowerOff(s) if t - s >= POWER_OFF => {
                if self.calm { Mode::Room } else { Mode::ZoomOut(t) }
            }
            Mode::ZoomOut(s) if t - s >= ZOOM_OUT => Mode::Room,
            m => m,
        };
        if matches!(self.mode, Mode::Room) && self.conn.is_none() {
            self.session = None;
        }

        // The TV's light in the room follows the highlighted menu item.
        let target = match (&self.page, self.sel) {
            (Page::Main, 0) | (Page::Host, _) => TV_COLD.lerp(pal::GOLD, 0.6),
            (Page::Main, 1) | (Page::Join | Page::Code | Page::Password, _) => TV_COLD.lerp(hue(1), 0.6),
            (Page::Main, 2) => TV_COLD.lerp(hue(4), 0.6),
            (Page::Main, _) | (Page::Error(_), _) => TV_COLD.lerp(pal::RED, 0.65),
            _ => TV_COLD,
        };
        self.glow = self.glow.lerp(target, 1.0 - (-dt * 9.0).exp());
        self.fx.flash *= (-dt * 7.0).exp();
        self.fx.shake *= (-dt * 9.0).exp();
    }

    fn now_est(&self) -> u64 {
        self.server_now + self.frame_at.elapsed().as_millis() as u64
    }

    fn me(&self) -> Pid {
        self.conn.as_ref().map(|c| c.me).unwrap_or(0)
    }

    fn send(&self, m: ClientMsg) {
        if let Some(c) = &self.conn {
            let _ = c.tx.send(m);
        }
    }

    fn on_frame(&mut self, now: u64, s: Session, ev: Vec<Event>) {
        let t = self.t();
        let tag = match s.phase {
            Phase::Lobby => 0,
            Phase::Intro { .. } => 1,
            Phase::Play(_) => 2,
            Phase::Results { .. } => 3,
            Phase::Final => 4,
        };
        if tag != self.phase_tag {
            if tag == 0 && self.phase_tag == 4 {
                self.appear.clear();
            }
            match tag {
                3 => self.sfx.play(Sound::Point),
                4 => {
                    self.sfx.play(Sound::Fanfare);
                    self.local = Local::default();
                    self.local.pt = t;
                }
                _ => {}
            }
            self.phase_tag = tag;
            self.phase_since = t;
        }
        for p in &s.players {
            if !self.appear.iter().any(|e| e.0 == p.id) {
                self.appear.push((p.id, t));
            }
        }
        if let Phase::Play(g) = &s.phase {
            let id = (s.round, g.kind());
            if self.game_id != Some(id) {
                self.game_id = Some(id);
                self.local = Local::default();
                self.local.pt = t;
                let me = self.me();
                let playing: Vec<Pid> = s.players.iter().filter(|p| !p.spectator).map(|p| p.id).collect();
                self.cam = if playing.contains(&me) { me } else { playing.first().copied().unwrap_or(me) };
            }
            g.observe(&mut self.local);
        }
        self.server_now = now;
        self.frame_at = Instant::now();
        self.session = Some(s);
        for e in ev {
            self.on_event(e);
        }
    }

    fn on_event(&mut self, e: Event) {
        let me = self.me();
        let t = self.t();
        match e {
            Event::Count(_) => self.sfx.play(Sound::Beep),
            Event::Go => {
                self.sfx.play(Sound::Go);
                self.go_t = t;
                self.fx.kick(if self.calm { 0.0 } else { 0.9 }, 0.25, pal::GOLD);
            }
            Event::Flash => self.sfx.play(Sound::Flash),
            Event::FalseStart(p) if p == me => self.sfx.play(Sound::Typo),
            Event::Reacted(p) if p == me => self.sfx.play(Sound::Point),
            Event::Crash(_) => self.sfx.play(Sound::Crash),
            Event::Finish(p) => self.sfx.play(if p == me { Sound::Fanfare } else { Sound::Point }),
            Event::Locked(p) => self.sfx.play(if p == me { Sound::Select } else { Sound::Tick }),
            Event::Reveal => self.sfx.play(Sound::Reveal),
            Event::Joined(_) if self.phase_tag == 0 => self.sfx.play(Sound::Join),
            _ => {}
        }
        let (Some(s), area) = (&self.session, self.area) else { return };
        if let Phase::Play(g) = &s.phase {
            let v = View { area: game_area(area), now: self.now_est(), t, me, cam: self.cam, players: &s.players, calm: self.calm };
            g.on_event(e, &v, &mut self.local, &mut self.fx);
        }
    }

    // ---------------- actions ----------------

    fn goto(&mut self, p: Page) {
        self.page = p;
        self.sel = 0;
        self.edit = None;
        self.chan_t = self.t();
        self.sfx.play(Sound::Static);
    }

    fn items(&self) -> Vec<(String, Item)> {
        let dots = |s: &str| if s.is_empty() { "none".to_string() } else { "•".repeat(s.chars().count()) };
        match &self.page {
            Page::Main => vec![
                ("HOST A ROOM".into(), Item::Host),
                ("JOIN A ROOM".into(), Item::Join),
                (format!("NAME  {}", self.name.to_uppercase()), Item::Name),
                ("QUIT".into(), Item::Quit),
            ],
            Page::Host => vec![
                (format!("ROOM  {}", self.room.to_uppercase()), Item::RoomName),
                (format!("PASSWORD  {}", dots(&self.password)), Item::Password),
                (format!("BOTS  ‹ {} ›", self.bots), Item::Bots),
                ("OPEN THE ROOM".into(), Item::Open),
                ("BACK".into(), Item::BackMain),
            ],
            Page::Join => {
                let mut v: Vec<(String, Item)> = self
                    .lan
                    .iter()
                    .enumerate()
                    .map(|(i, r)| (format!("{}{}", r.name.to_uppercase(), if r.locked { "  [locked]" } else { "" }), Item::Room(i)))
                    .collect();
                v.push(("ENTER AN INVITE CODE".into(), Item::Code));
                v.push(("BACK".into(), Item::BackMain));
                v
            }
            Page::Code => vec![
                (format!("CODE  {}", if self.code.is_empty() { "paste it here".into() } else { short(&self.code) }), Item::Code),
                ("TUNE IN".into(), Item::CodeGo),
                ("BACK".into(), Item::BackJoin),
            ],
            Page::Password => vec![
                (format!("PASSWORD  {}", dots(&self.join_pw)), Item::JoinPw),
                ("TUNE IN".into(), Item::PwGo),
                ("BACK".into(), Item::BackJoin),
            ],
            Page::Connecting(_) => vec![],
            Page::Error(_) => vec![("OK".into(), Item::Ok)],
        }
    }

    fn activate(&mut self, it: Item) {
        self.sfx.play(Sound::Select);
        match it {
            Item::Host => self.goto(Page::Host),
            Item::Join => self.goto(Page::Join),
            Item::Name => self.edit = Some((Field::Name, self.name.clone())),
            Item::Quit => self.quit = true,
            Item::RoomName => self.edit = Some((Field::Room, self.room.clone())),
            Item::Password => self.edit = Some((Field::Password, self.password.clone())),
            Item::Bots => self.bots = (self.bots + 1) % 8,
            Item::Open => self.open_room(),
            Item::BackMain => self.goto(Page::Main),
            Item::BackJoin => self.goto(Page::Join),
            Item::Room(i) => {
                let Some(r) = self.lan.get(i).cloned() else { return };
                self.target = Some(r.id);
                if r.locked {
                    self.goto(Page::Password);
                    self.edit = Some((Field::JoinPw, String::new()));
                } else {
                    self.join(r.id, String::new(), r.name);
                }
            }
            Item::Code if self.page == Page::Join => {
                self.goto(Page::Code);
                self.edit = Some((Field::Code, self.code.clone()));
            }
            Item::Code => self.edit = Some((Field::Code, self.code.clone())),
            Item::CodeGo => match self.code.trim().parse::<EndpointId>() {
                Ok(id) => self.join(id, self.join_pw.clone(), "THE ROOM".into()),
                Err(_) => self.goto(Page::Error("That invite code doesn't look right. Copy it again from the host's lobby.".into())),
            },
            Item::JoinPw => self.edit = Some((Field::JoinPw, self.join_pw.clone())),
            Item::PwGo => {
                if let Some(id) = self.target {
                    self.join(id, self.join_pw.clone(), "THE ROOM".into());
                }
            }
            Item::Ok => self.goto(Page::Main),
        }
    }

    fn commit_edit(&mut self, f: Field, v: String) {
        let v = v.trim().to_string();
        match f {
            Field::Name if !v.is_empty() => self.name = v,
            Field::Room if !v.is_empty() => self.room = v,
            Field::Password => self.password = v,
            Field::Code => self.code = v,
            Field::JoinPw => self.join_pw = v,
            _ => {}
        }
    }

    fn open_room(&mut self) {
        let code = self.net.as_ref().map(|n| n.code()).unwrap_or_else(|| "offline".into());
        let server = server::spawn(self.room.clone(), code, self.password.clone(), self.bots);
        let accept = self.net.as_ref().and_then(|n| n.host(&self.room, !self.password.is_empty(), server.clone()).ok());
        let (tx, rx) = oneshot::channel();
        let (name, pw) = (self.name.clone(), self.password.clone());
        let hosting = Hosting { accept, server: server.clone() };
        self.rt.spawn(async move {
            let r = net::local(&server, name, pw).await.map(|c| (c, Some(hosting)));
            let _ = tx.send(r);
        });
        self.pending = Some(rx);
        self.goto(Page::Connecting("OPENING THE STUDIO".into()));
    }

    fn join(&mut self, id: EndpointId, pw: String, label: String) {
        let Some(net) = &self.net else {
            let why = self.net_err.clone().unwrap_or_else(|| "The network is still warming up. Try again in a second.".into());
            self.goto(Page::Error(why));
            return;
        };
        let (tx, rx) = oneshot::channel();
        let (ep, name) = (net.ep.clone(), self.name.clone());
        self.rt.spawn(async move {
            let r = net::Net::join(ep, id, name, pw).await.map(|c| (c, None));
            let _ = tx.send(r);
        });
        self.pending = Some(rx);
        self.goto(Page::Connecting(format!("TUNING IN TO {}", label.to_uppercase())));
    }

    fn shutdown_hosting(&mut self) {
        if let Some(h) = self.hosting.take() {
            if let Some(a) = h.accept {
                a.abort();
            }
            let _ = h.server.send(Cmd::Shutdown);
            if let Some(n) = &self.net {
                n.stop_hosting();
            }
        }
    }

    fn leave(&mut self, why: Option<String>) {
        self.conn = None;
        self.shutdown_hosting();
        self.game_id = None;
        let t = self.t();
        self.mode = if self.calm || self.session.is_none() { Mode::Room } else { Mode::PowerOff(t) };
        self.sfx.play(Sound::Back);
        match why {
            Some(w) => self.goto(Page::Error(w)),
            None => self.goto(Page::Main),
        }
    }

    // ---------------- input ----------------

    fn on_input(&mut self, e: Ct) {
        match e {
            Ct::Key(k) if k.kind != KeyEventKind::Release => {
                if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
                    self.quit = true;
                    return;
                }
                match self.mode {
                    Mode::Room => self.menu_key(k),
                    Mode::Studio(_) => self.studio_key(k),
                    _ => {}
                }
            }
            Ct::Paste(s) => {
                if let Some((_, buf)) = &mut self.edit {
                    buf.extend(s.chars().filter(|c| !c.is_control()).take(120));
                }
            }
            _ => {}
        }
    }

    fn menu_key(&mut self, k: KeyEvent) {
        if let Some((field, buf)) = &mut self.edit {
            let max = match field {
                Field::Name => 12,
                Field::Room => 20,
                Field::Password | Field::JoinPw => 24,
                Field::Code => 120,
            };
            match k.code {
                KeyCode::Char(c) if buf.chars().count() < max => buf.push(c),
                KeyCode::Backspace => {
                    buf.pop();
                }
                KeyCode::Enter => {
                    let (f, v) = (*field, buf.clone());
                    self.edit = None;
                    self.commit_edit(f, v);
                    self.sfx.play(Sound::Select);
                }
                KeyCode::Esc => self.edit = None,
                _ => {}
            }
            return;
        }
        let items = self.items();
        let n = items.len().max(1);
        match k.code {
            KeyCode::Up | KeyCode::Char('w') | KeyCode::Char('k') => {
                self.sel = (self.sel + n - 1) % n;
                self.sfx.play(Sound::Tick);
            }
            KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('j') | KeyCode::Tab => {
                self.sel = (self.sel + 1) % n;
                self.sfx.play(Sound::Tick);
            }
            KeyCode::Left | KeyCode::Right if items.get(self.sel).map(|i| i.1) == Some(Item::Bots) => {
                self.bots = if k.code == KeyCode::Left { (self.bots + 7) % 8 } else { (self.bots + 1) % 8 };
                self.sfx.play(Sound::Tick);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(&(_, it)) = items.get(self.sel) {
                    self.activate(it);
                }
            }
            KeyCode::Esc => match self.page {
                Page::Main => {}
                Page::Code | Page::Password => self.goto(Page::Join),
                Page::Connecting(_) => {
                    self.pending = None;
                    self.shutdown_hosting();
                    self.goto(Page::Main);
                }
                _ => self.goto(Page::Main),
            },
            _ => {}
        }
        self.sel = self.sel.min(self.items().len().saturating_sub(1));
    }

    fn studio_key(&mut self, k: KeyEvent) {
        let Some(s) = &self.session else {
            if k.code == KeyCode::Esc {
                self.leave(None);
            }
            return;
        };
        let me = self.me();
        let host = self.hosting.is_some();
        let mine = s.player(me).cloned();
        let t = self.t();
        match &s.phase {
            Phase::Lobby => match k.code {
                KeyCode::Char(' ') => {
                    self.send(ClientMsg::Ready(!mine.map(|p| p.ready).unwrap_or(false)));
                    self.sfx.play(Sound::Select);
                }
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    self.send(ClientMsg::Spectate(!mine.map(|p| p.spectator).unwrap_or(false)));
                    self.sfx.play(Sound::Tick);
                }
                KeyCode::Left if host => self.send(ClientMsg::Rounds(s.rounds.saturating_sub(1))),
                KeyCode::Right if host => self.send(ClientMsg::Rounds(s.rounds + 1)),
                KeyCode::Enter if host => self.send(ClientMsg::Start),
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    osc52(&s.code);
                    self.toast = Some(("INVITE CODE COPIED".into(), t));
                    self.sfx.play(Sound::Point);
                }
                KeyCode::Esc => self.leave(None),
                _ => {}
            },
            Phase::Final => match k.code {
                KeyCode::Enter if host => self.send(ClientMsg::ToLobby),
                KeyCode::Esc => self.leave(None),
                _ => {}
            },
            Phase::Play(g) => {
                let playing = mine.as_ref().is_some_and(|p| !p.spectator) && participates(g, me);
                if !playing {
                    if matches!(k.code, KeyCode::Tab | KeyCode::Right | KeyCode::Left) {
                        let ids: Vec<Pid> = s.players.iter().filter(|p| !p.spectator).map(|p| p.id).collect();
                        if let Some(i) = ids.iter().position(|&p| p == self.cam) {
                            let j = if k.code == KeyCode::Left { (i + ids.len() - 1) % ids.len() } else { (i + 1) % ids.len() };
                            self.cam = ids[j];
                        } else if let Some(&p) = ids.first() {
                            self.cam = p;
                        }
                        self.sfx.play(Sound::Tick);
                    }
                    return;
                }
                let input = match (g, k.code) {
                    (Game::Blink(b), KeyCode::Char(' ') | KeyCode::Enter) => b.press(&mut self.local),
                    (Game::Grid(_), code) => dir_of(code).map(Input::Dir),
                    (Game::Odd(o), KeyCode::Char(c @ '1'..='3')) => {
                        let round = o.round;
                        if self.local.picked.map(|p| p.0) == Some(round) || o.picks.iter().any(|p| p.0 == me && p.1.is_some()) {
                            None
                        } else {
                            let k = c as u8 - b'1';
                            self.local.picked = Some((round, k));
                            Some(Input::Pick(k))
                        }
                    }
                    (Game::Derby(_), KeyCode::Backspace) => {
                        self.local.typed.pop();
                        None
                    }
                    (Game::Derby(d), KeyCode::Char(c)) if c.is_ascii_alphabetic() => match d.key(&mut self.local, me, c) {
                        derby::Key::Word(i) => {
                            self.sfx.play(Sound::Word);
                            Some(i)
                        }
                        derby::Key::Typo => {
                            self.sfx.play(Sound::Typo);
                            self.local.typo_t = t;
                            if !self.calm {
                                self.fx.kick(0.25, 0.0, pal::RED);
                            }
                            None
                        }
                        derby::Key::Ok => {
                            self.sfx.play(Sound::Tick);
                            None
                        }
                        derby::Key::Ignore => None,
                    },
                    _ => None,
                };
                if let Some(i) = input {
                    self.send(ClientMsg::Input(i));
                }
            }
            _ => {}
        }
    }

    // ---------------- drawing ----------------

    fn draw(&mut self, f: &mut Frame) {
        let a = f.area();
        self.area = a;
        let buf = f.buffer_mut();
        if a.width < 80 || a.height < 24 {
            gfx::Canvas::for_area(a, pal::STAGE_DEEP).blit(buf, a);
            ctext(buf, a, a.y as i32 + a.height as i32 / 2 - 1, "PARTYLINE needs a window of at least 80 x 24.", pal::INK);
            ctext(buf, a, a.y as i32 + a.height as i32 / 2 + 1, &format!("Yours is {} x {}. Make it a little bigger.", a.width, a.height), pal::DIM);
            return;
        }
        let t = self.t();
        match self.mode {
            Mode::Room => self.draw_room(buf, a, 0.0, true),
            Mode::ZoomIn(s) => self.draw_room(buf, a, ease_in_out(prog(t, s, ZOOM_IN)), false),
            Mode::ZoomOut(s) => {
                let p = ease_in_out(prog(t, s, ZOOM_OUT));
                self.draw_room(buf, a, 1.0 - p, p > 0.9);
            }
            Mode::Studio(s) => {
                self.draw_studio(buf, a, t);
                studio::crt(buf, a, prog(t, s, POWER_ON), true);
            }
            Mode::PowerOff(s) => {
                self.draw_studio(buf, a, t);
                studio::crt(buf, a, prog(t, s, POWER_OFF), false);
            }
        }
    }

    /// The room, `p` of the way along the dolly into the TV; `menu` draws the TV menu on the glass.
    fn draw_room(&mut self, buf: &mut Buffer, a: Rect, p: f32, menu: bool) {
        let t = self.t();
        let mut shot = room::draw(a, p, t, self.glow);
        let sr = shot.screen(a);
        // Logo rows on the glass: 0 = none, else the display-font scale.
        let logo = if !menu || sr.width < 24 || sr.height < 10 { 0 } else if sr.height >= 20 && sr.width >= 44 { 2 } else { 1 };
        if logo > 0 {
            let (ox, oy) = ((sr.x - a.x) as i32, (sr.y - a.y) as i32 * 2);
            let w = big_width("PARTYLINE", logo);
            shot.cv.big_center(ox + sr.width as i32 / 2, oy + 4, "PARTYLINE", logo, &|x, y| {
                let sweep = ((t * 0.4).fract() * (w + 30) as f32) as i32 - 15;
                let c = pal::INK.lerp(pal::GOLD, y as f32 / (7 * logo) as f32);
                if (x + y / 2 - sweep).abs() < 2 { c.lerp(pal::INK, 0.7) } else { c }
            });
        }
        shot.cv.blit(buf, a);
        if menu && sr.width > 10 && sr.height > 6 {
            self.draw_menu(buf, sr, t, if logo > 0 { (4 + 7 * logo) / 2 + 1 } else { 0 });
        }
    }

    /// The TV menu inside the glass rect `sr`; `logo_rows` are already taken by the logo.
    fn draw_menu(&mut self, buf: &mut Buffer, sr: Rect, t: f32, logo_rows: i32) {
        // Channel-change static.
        if t - self.chan_t < 0.14 {
            let seed = (t * 120.0) as u32;
            for y in sr.top()..sr.bottom() {
                for x in sr.left()..sr.right() {
                    let h = hash(seed ^ x as u32 * 13, y as u32);
                    let g = (h % 100) as f32 / 100.0;
                    let c = &mut buf[(x, y)];
                    c.set_symbol(["░", "▒", "▓", " "][(h >> 8) as usize % 4]);
                    c.fg = Rgb(g, g, g).color();
                    c.bg = Rgb(g * 0.3, g * 0.3, g * 0.35).color();
                }
            }
            return;
        }
        let top = sr.y as i32;
        text(buf, sr, sr.x as i32 + 3, top, "CH 03", pal::DIM);
        let live = if (t * 1.4).fract() < 0.6 { pal::RED } else { pal::PODIUM_DARK };
        text(buf, sr, sr.right() as i32 - 9, top, "● REC", live);
        let mut y = top + 1;
        if logo_rows > 0 {
            y = top + logo_rows;
        } else {
            ctext(buf, sr, y + 1, "P A R T Y L I N E", pal::INK);
            y += 3;
        }
        match &self.page {
            Page::Connecting(msg) => {
                let dots = ".".repeat(((t * 3.0) as usize % 3) + 1);
                ctext(buf, sr, y + 2, &format!("{msg}{dots}"), pal::INK);
                ctext(buf, sr, y + 4, "ESC cancels", pal::DIM);
                return;
            }
            Page::Error(msg) => {
                for (i, line) in wrap(msg, sr.width as usize - 6).iter().enumerate() {
                    ctext(buf, sr, y + 1 + i as i32, line, pal::INK);
                }
                y += wrap(msg, sr.width as usize - 6).len() as i32 + 2;
            }
            Page::Join if self.lan.is_empty() => {
                let dots = ".".repeat(((t * 2.0) as usize % 3) + 1);
                ctext(buf, sr, y, &format!("Looking for rooms on your network{dots}"), pal::DIM);
                y += 2;
            }
            _ => {}
        }
        let items = self.items();
        let avail = (sr.bottom() as i32 - 2 - y).max(1) as usize;
        let first = self.sel.saturating_sub(avail.saturating_sub(1));
        for (i, (label, _)) in items.iter().enumerate().skip(first).take(avail) {
            let selected = i == self.sel;
            let editing = selected && self.edit.is_some();
            let label = match (&self.edit, editing) {
                (Some((f, buf_)), true) => {
                    let shown = if matches!(f, Field::Password | Field::JoinPw) { "•".repeat(buf_.chars().count()) } else { buf_.to_uppercase() };
                    let caret = if (t * 2.0).fract() < 0.5 { "▏" } else { " " };
                    let head = label.split("  ").next().unwrap_or("");
                    format!("{head}  {}{caret}", tail(&shown, sr.width as usize - head.len() - 10))
                }
                _ => label.clone(),
            };
            if selected {
                let pill = format!("  {label}  ");
                let x = sr.x as i32 + (sr.width as i32 - pill.chars().count() as i32) / 2;
                text_bg(buf, sr, x, y, &pill, pal::STAGE_DEEP, if editing { pal::INK } else { pal::GOLD });
            } else {
                ctext(buf, sr, y, &label, pal::INK.lerp(pal::DIM, 0.4));
            }
            y += 1;
        }
        let hint = if self.edit.is_some() {
            "type · ENTER save · ESC cancel".to_string()
        } else if let Some(e) = &self.net_err {
            format!("offline: {}", e.chars().take(sr.width as usize - 12).collect::<String>())
        } else if self.net.is_none() {
            "warming up the network…".to_string()
        } else {
            "↑↓ choose · ENTER select".to_string()
        };
        ctext(buf, sr, sr.bottom() as i32 - 1, &hint, pal::DIM);
    }

    fn draw_studio(&mut self, buf: &mut Buffer, a: Rect, t: f32) {
        let me = self.me();
        let host = self.hosting.is_some();
        let now = self.now_est();
        let Some(s) = &self.session else {
            let mut cv = crate::games::stage(a, t, 0.0);
            let cx = cv.w / 2;
            cv.big_center(cx, cv.h / 2 - 4, "STAND BY", 1, &|_, _| pal::DIM);
            cv.blit(buf, a);
            return;
        };
        match &s.phase {
            Phase::Lobby => {
                // Podiums rise only once the screen is on, not while the camera is still flying in.
                let on = self.studio_t + POWER_ON * 0.5;
                let appear: Vec<(Pid, f32)> = self.appear.iter().map(|&(p, at)| (p, at.max(on))).collect();
                studio::lobby(buf, a, s, me, host, t, &appear)
            }
            Phase::Intro { kind, go_at } => studio::intro(buf, a, s, *kind, *go_at, now, t, self.phase_since),
            Phase::Play(g) => {
                let v = View { area: game_area(a), now, t, me, cam: self.cam, players: &s.players, calm: self.calm };
                g.render(&v, &mut self.local, buf);
                studio::hud(buf, a, s, g.kind());
                let playing = s.player(me).is_some_and(|p| !p.spectator) && participates(g, me);
                if !playing {
                    if let Some(p) = s.player(self.cam) {
                        studio::lower_third(buf, a, &p.name.to_uppercase(), hue(p.hue), t);
                    }
                }
                if t - self.go_t < 0.6 && !self.calm {
                    studio::go(buf, a, prog(t, self.go_t, 0.6));
                }
            }
            Phase::Results { kind, gains, until } => studio::results(buf, a, s, *kind, gains, *until, now, t, self.phase_since),
            Phase::Final => studio::finale(buf, a, s, host, t, self.phase_since, &mut self.local),
        }
        if let Some((msg, t0)) = &self.toast {
            let k = t - t0;
            if k < 1.6 {
                let pill = format!("  {msg}  ");
                let x = a.x as i32 + (a.width as i32 - pill.len() as i32) / 2;
                let y = a.bottom() as i32 - 5 + if k < 0.2 { ((0.2 - k) * 10.0) as i32 } else { 0 };
                text_bg(buf, a, x, y, &pill, pal::STAGE_DEEP, pal::GOLD);
            }
        }
        gfx::tint(buf, a, self.fx.flash_c, self.fx.flash);
        if self.fx.shake > 0.05 {
            let h = hash((t * 60.0) as u32, 99);
            let dx = ((h % 3) as i32 - 1) * (self.fx.shake * 1.5).round() as i32;
            let dy = (((h >> 4) % 3) as i32 - 1) * (self.fx.shake * 0.8).round() as i32;
            gfx::shift(buf, a, dx, dy);
        }
        gfx::crt_glass(buf, a, t);
    }
}

fn game_area(a: Rect) -> Rect {
    Rect::new(a.x, a.y + 1, a.width, a.height.saturating_sub(1))
}

fn participates(g: &Game, me: Pid) -> bool {
    match g {
        Game::Blink(s) => s.rt.iter().any(|e| e.0 == me),
        Game::Grid(s) => s.bikes.iter().any(|b| b.p == me),
        Game::Derby(s) => s.progress.iter().any(|e| e.0 == me),
        Game::Odd(s) => s.picks.iter().any(|e| e.0 == me),
    }
}

fn dir_of(code: KeyCode) -> Option<Dir> {
    match code {
        KeyCode::Up | KeyCode::Char('w') | KeyCode::Char('W') => Some(Dir::Up),
        KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('S') => Some(Dir::Down),
        KeyCode::Left | KeyCode::Char('a') | KeyCode::Char('A') => Some(Dir::Left),
        KeyCode::Right | KeyCode::Char('d') | KeyCode::Char('D') => Some(Dir::Right),
        _ => None,
    }
}

fn short(code: &str) -> String {
    if code.len() > 16 { format!("{}…{}", &code[..8], &code[code.len() - 6..]) } else { code.to_string() }
}

fn tail(s: &str, max: usize) -> String {
    let n = s.chars().count();
    s.chars().skip(n.saturating_sub(max.max(4))).collect()
}

fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in s.split_whitespace() {
        let cur = lines.last_mut().unwrap();
        if !cur.is_empty() && cur.len() + word.len() + 1 > width {
            lines.push(word.to_string());
        } else {
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(word);
        }
    }
    lines
}

/// Turn network errors into something a player can act on.
fn friendly(e: &anyhow::Error) -> String {
    let raw = e.to_string();
    if raw.contains("Wrong password") || raw.contains("full") || raw.contains("Timed out") {
        return raw;
    }
    format!("Couldn't reach that room. Check the code, and that the host is still in the lobby. ({})", raw.chars().take(60).collect::<String>())
}

/// Copy to the system clipboard through the terminal (OSC 52; Windows Terminal, kitty, WezTerm, iTerm2).
fn osc52(s: &str) {
    const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in s.as_bytes().chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    let mut so = std::io::stdout();
    let _ = write!(so, "\x1b]52;c;{out}\x07");
    let _ = so.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dev-only visual QA: renders every screen to target/snap/*.html.
    /// Run with `cargo test snapshots -- --ignored`.
    #[test]
    #[ignore]
    fn snapshots() {
        use crate::games::{blink, gridlock, odd};
        use crate::proto::{Link, Player};

        let rt = tokio::runtime::Runtime::new().unwrap();
        let mut app = App::new(rt.handle().clone(), Args { name: "LUAN".into(), bots: 3, calm: false, mute: true });
        let dir = std::path::Path::new("target/snap");
        std::fs::create_dir_all(dir).unwrap();
        let save = |name: &str, buf: &Buffer| std::fs::write(dir.join(format!("{name}.html")), html(buf)).unwrap();

        for (w, h) in [(140u16, 40u16), (100, 30)] {
            let a = Rect::new(0, 0, w, h);
            let tag = format!("{w}x{h}");
            let mut buf = Buffer::empty(a);
            app.glow = TV_COLD.lerp(pal::GOLD, 0.6);
            app.draw_room(&mut buf, a, 0.0, true);
            save(&format!("room_{tag}"), &buf);
            let mut buf = Buffer::empty(a);
            app.page = Page::Join;
            app.glow = TV_COLD.lerp(hue(1), 0.6);
            app.draw_room(&mut buf, a, 0.0, true);
            save(&format!("room_join_{tag}"), &buf);
            app.page = Page::Main;
            let mut buf = Buffer::empty(a);
            app.draw_room(&mut buf, a, 0.6, false);
            save(&format!("zoom_{tag}"), &buf);

            let p = |id, name: &str, hue, link, ready, spectator, score| Player { id, name: name.into(), hue, ready, spectator, score, link, ping: 12 };
            let players = vec![
                p(1, "luan", 0, Link::Host, true, false, 9),
                p(2, "mari", 1, Link::Lan, true, false, 6),
                p(3, "joao", 2, Link::Relay, false, false, 4),
                p(4, "ROBO", 3, Link::Bot, true, false, 2),
                p(5, "bia", 4, Link::Direct, false, true, 0),
            ];
            let mut s = Session { room: "luan's room".into(), code: "a".repeat(52), players, phase: Phase::Lobby, round: 1, rounds: 4 };
            let ids = [1, 2, 3, 4];
            let shot = |name: &str, s: &Session, f: &mut dyn FnMut(&mut Buffer, &Session)| {
                let mut buf = Buffer::empty(a);
                f(&mut buf, s);
                gfx::crt_glass(&mut buf, a, 10.0);
                save(&format!("{name}_{tag}"), &buf);
            };
            shot("lobby", &s, &mut |b, s| studio::lobby(b, a, s, 1, true, 10.0, &[]));
            shot("intro", &s, &mut |b, s| studio::intro(b, a, s, Kind::Gridlock, 5000, 0, 10.0, 0.0));
            shot("count", &s, &mut |b, s| studio::intro(b, a, s, Kind::Gridlock, 2050, 0, 10.0, 0.0));

            let mut l = Local::default();
            fn v(a: Rect, players: &[Player]) -> View<'_> {
                View { area: game_area(a), now: 8000, t: 10.0, me: 1, cam: 1, players, calm: false }
            }
            let mut g = blink::S::new(&ids, 1, 0);
            let mut ev = vec![];
            let mut now = 0;
            while !matches!(g.stage, blink::Stage::Flash { .. }) {
                now += 16;
                g.tick(now, &mut ev);
            }
            g.input(2, &Input::React(231), &mut ev);
            g.input(3, &Input::Early, &mut ev);
            shot("blink", &s, &mut |b, s| {
                g.render(&v(a, &s.players), &mut l, b);
                studio::hud(b, a, s, Kind::Blink);
            });

            let mut g = gridlock::S::new(&ids, 1, 0);
            for now in (0..9000).step_by(16) {
                for &p in &ids {
                    if let Some(i) = g.bot(p) {
                        g.input(p, &i);
                    }
                }
                g.tick(now, &mut ev);
            }
            shot("gridlock", &s, &mut |b, s| {
                g.render(&v(a, &s.players), &mut l, b);
                studio::hud(b, a, s, Kind::Gridlock);
            });

            let mut g = crate::games::derby::S::new(&ids, 2, 0);
            for (p, n) in [(1u8, 5u16), (2, 8), (3, 3), (4, 6)] {
                for i in 0..n {
                    g.input(p, &Input::Word(i), 0, &mut ev);
                }
            }
            let mut l2 = Local { typed: g.words[5][..2].to_string(), ..Local::default() };
            l2.race = vec![(1, 5.0 / 16.0), (2, 0.5), (3, 3.0 / 16.0), (4, 6.0 / 16.0)];
            l2.pt = 10.0;
            shot("derby", &s, &mut |b, s| {
                g.render(&v(a, &s.players), &mut l2, b);
                studio::hud(b, a, s, Kind::Derby);
            });

            let mut g = odd::S::new(&ids, 3, 0);
            for (p, k) in [(1u8, 0u8), (2, 0), (3, 2), (4, 1)] {
                g.input(p, &Input::Pick(k), &mut ev);
            }
            g.tick(1, &mut ev);
            let mut l3 = Local { reveal_t: 0.0, ..Local::default() };
            shot("odd", &s, &mut |b, s| {
                g.render(&v(a, &s.players), &mut l3, b);
                studio::hud(b, a, s, Kind::Odd);
            });

            let gains = vec![(1, 4), (2, 2), (4, 1), (3, 0)];
            shot("results", &s, &mut |b, s| studio::results(b, a, s, Kind::Blink, &gains, 4000, 0, 10.0, 0.0));
            s.phase = Phase::Final;
            let mut l4 = Local::default();
            shot("final", &s, &mut |b, s| studio::finale(b, a, s, true, 10.0, 0.0, &mut l4));
            let mut buf = Buffer::empty(a);
            studio::lobby(&mut buf, a, &s, 1, true, 10.0, &[]);
            studio::go(&mut buf, a, 0.3);
            save(&format!("go_{tag}"), &buf);
        }
        // One page with every screen, for a single scroll-through review.
        let mut names: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().into_string().unwrap()).filter(|n| n != "gallery.html").collect();
        names.sort();
        let mut g = String::from("<!doctype html><meta charset=utf-8><body style='background:#111;color:#aaa;font:12px sans-serif'>");
        for n in names {
            let body = std::fs::read_to_string(dir.join(&n)).unwrap();
            let pre = &body[body.find("<pre").unwrap()..];
            g.push_str(&format!("<h3>{n}</h3>{pre}"));
        }
        std::fs::write(dir.join("gallery.html"), g).unwrap();
    }

    fn html(buf: &Buffer) -> String {
        let hex = |c: ratatui::style::Color| {
            let r = Rgb::from_color(c);
            format!("#{:02x}{:02x}{:02x}", (r.0 * 255.0) as u8, (r.1 * 255.0) as u8, (r.2 * 255.0) as u8)
        };
        let px = (1180.0 / (buf.area.width as f32 * 0.6)).min(15.0);
        let mut s = format!(
            "<!doctype html><meta charset=utf-8><body style='background:#000;margin:4px'><pre style='font:{px:.1}px/1 \"Cascadia Mono\",Consolas,monospace;margin:0'>"
        );
        for y in 0..buf.area.height {
            let mut run = (String::new(), String::new(), String::new());
            for x in 0..buf.area.width {
                let c = &buf[(x, y)];
                let sym = match c.symbol() {
                    "<" => "&lt;",
                    ">" => "&gt;",
                    "&" => "&amp;",
                    other => other,
                };
                let (fg, bg) = (hex(c.fg), hex(c.bg));
                if (fg.as_str(), bg.as_str()) != (run.0.as_str(), run.1.as_str()) {
                    if !run.2.is_empty() {
                        s.push_str(&format!("<span style='color:{};background:{}'>{}</span>", run.0, run.1, run.2));
                    }
                    run = (fg, bg, String::new());
                }
                run.2.push_str(sym);
            }
            s.push_str(&format!("<span style='color:{};background:{}'>{}</span>\n", run.0, run.1, run.2));
        }
        s + "</pre>"
    }

    #[test]
    fn wrap_respects_width() {
        let lines = wrap("the quick brown fox jumps over the lazy dog", 12);
        assert!(lines.iter().all(|l| l.len() <= 12));
        assert_eq!(lines.join(" "), "the quick brown fox jumps over the lazy dog");
    }
}
