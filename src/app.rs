//! The app: living-room TV menu, camera dolly into the screen, the studio session, input.
use std::io::Write as _;
use std::time::{Duration, Instant};

use iroh::EndpointId;
use ratatui::crossterm::event::{self, Event as Ct, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, Frame, buffer::Buffer, layout::Rect};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::games::{Fx, Game, Local, View, derby};
use crate::gfx::{
    self, Canvas, Hint, LINE, Rgb, SMALL, big_width, ease_in_out, ease_out, fit_label, hash, hue, label_width, pal, prog, tail_label, type_h, type_width,
    wrap_label,
};
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
    pub update: bool,
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
    /// Headline, then what it is connecting to.
    Connecting(String, String),
    /// Headline, then a short line on what to do about it.
    Error(String, String),
}

/// Room the TV glass must have for the menu, in canvas pixels; small windows move the camera
/// closer until it does.
const MENU_W: f32 = 60.0;
const MENU_H: f32 = 40.0;

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
    /// When the highlight last moved, and which way (-1 up, 1 down): the channel rolls in.
    sel_t: f32,
    sel_dir: i32,
    edit: Option<(Field, String)>,
    chan_t: f32,
    room: String,
    password: String,
    bots: u8,
    code: String,
    join_pw: String,
    target: Option<(EndpointId, String)>,
    lan: Vec<RoomAd>,
    lan_rx: Option<mpsc::UnboundedReceiver<Lan>>,
    net: Option<Net>,
    net_rx: Option<oneshot::Receiver<anyhow::Result<Net>>>,
    net_err: Option<String>,
    update_rx: std::sync::mpsc::Receiver<String>,
    /// A newer release already swapped in; it runs on the next launch.
    updated: Option<String>,
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
            sel_t: -1.0,
            sel_dir: 0,
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
            update_rx: crate::update::start(args.update),
            updated: None,
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

        if let Ok(tag) = self.update_rx.try_recv() {
            self.updated = Some(tag);
        }
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
        // A room that went away can take the highlighted channel with it.
        self.sel = self.sel.min(self.items().len().saturating_sub(1));
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
                    self.fail(friendly(&e));
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
            self.leave(Some(("OFF THE AIR", "the line to the host dropped")));
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
            (Page::Main, _) | (Page::Error(..), _) => TV_COLD.lerp(pal::RED, 0.65),
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
        self.sel_t = self.t();
        self.sel_dir = 0;
        self.edit = None;
        self.chan_t = self.t();
        self.sfx.play(Sound::Static);
    }

    fn fail(&mut self, (head, why): (&str, &str)) {
        self.goto(Page::Error(head.into(), why.into()));
    }

    /// The channels on the current page, by name (what the neighbours on the wheel show).
    fn items(&self) -> Vec<(String, Item)> {
        let s = |l: &str, it: Item| (l.to_string(), it);
        match &self.page {
            Page::Main => vec![s("HOST A ROOM", Item::Host), s("JOIN A ROOM", Item::Join), s("YOUR NAME", Item::Name), s("QUIT", Item::Quit)],
            Page::Host => vec![
                s("ROOM NAME", Item::RoomName),
                s("PASSWORD", Item::Password),
                s("BOTS", Item::Bots),
                s("GO LIVE", Item::Open),
                s("BACK", Item::BackMain),
            ],
            Page::Join => {
                let mut v: Vec<(String, Item)> = self.lan.iter().enumerate().map(|(i, r)| (r.name.to_uppercase(), Item::Room(i))).collect();
                v.push(s("INVITE CODE", Item::Code));
                v.push(s("BACK", Item::BackMain));
                v
            }
            Page::Code => vec![s("INVITE CODE", Item::Code), s("TUNE IN", Item::CodeGo), s("BACK", Item::BackJoin)],
            Page::Password => vec![s("PASSWORD", Item::JoinPw), s("TUNE IN", Item::PwGo), s("BACK", Item::BackJoin)],
            Page::Connecting(..) => vec![],
            Page::Error(..) => vec![s("OK", Item::Ok)],
        }
    }

    /// A channel on air: its headline (a setting shows its value, dim when it is a
    /// placeholder) and what its caption adds after the channel number.
    fn channel(&self, it: Item, name: &str) -> (String, bool, Option<String>) {
        let setting = |v: String, empty: &str| (if v.is_empty() { empty.to_string() } else { v.clone() }, v.is_empty(), Some(name.to_string()));
        let stars = |s: &str| "*".repeat(s.chars().count());
        match it {
            Item::Name => setting(self.name.to_uppercase(), "NOBODY"),
            Item::RoomName => setting(self.room.to_uppercase(), "UNNAMED"),
            Item::Password => setting(stars(&self.password), "NONE"),
            Item::Bots => (format!("< {} >", self.bots), false, Some(name.to_string())),
            Item::Code if self.page == Page::Code => setting(if self.code.is_empty() { String::new() } else { short(&self.code) }, "PASTE IT"),
            Item::JoinPw => setting(stars(&self.join_pw), "TYPE IT"),
            Item::Room(i) if self.lan.get(i).is_some_and(|r| r.locked) => (name.to_string(), false, Some("LOCKED".into())),
            _ => (name.to_string(), false, None),
        }
    }

    /// Pages that wear the PARTYLINE logo; the rest get a small title.
    fn branded(&self) -> bool {
        matches!(self.page, Page::Main | Page::Connecting(..) | Page::Error(..))
    }

    fn page_title(&self) -> &'static str {
        match self.page {
            Page::Host => "HOST A ROOM",
            Page::Join => "JOIN A ROOM",
            Page::Code => "JOIN BY CODE",
            Page::Password => "LOCKED ROOM",
            _ => "PARTYLINE",
        }
    }

    /// A line under the small title.
    fn status(&self) -> Option<String> {
        match &self.page {
            Page::Join if self.lan.is_empty() => Some("scanning your network…".into()),
            Page::Join if self.lan.len() == 1 => Some("1 room on your network".into()),
            Page::Join => Some(format!("{} rooms on your network", self.lan.len())),
            Page::Password => self.target.as_ref().map(|r| r.1.to_uppercase()),
            _ => None,
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
                self.target = Some((r.id, r.name.clone()));
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
                Ok(id) => self.join(id, self.join_pw.clone(), "BY INVITE CODE".into()),
                Err(_) => self.fail(("BAD CODE", "copy it again from the lobby")),
            },
            Item::JoinPw => self.edit = Some((Field::JoinPw, self.join_pw.clone())),
            Item::PwGo => {
                if let Some((id, name)) = self.target.clone() {
                    self.join(id, self.join_pw.clone(), name);
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
        self.goto(Page::Connecting("GOING LIVE".into(), self.room.to_uppercase()));
    }

    fn join(&mut self, id: EndpointId, pw: String, label: String) {
        let Some(net) = &self.net else {
            self.fail(if self.net_err.is_some() { ("OFFLINE", "the network didn't start") } else { ("NOT YET", "the network is warming up") });
            return;
        };
        let (tx, rx) = oneshot::channel();
        let (ep, name) = (net.ep.clone(), self.name.clone());
        self.rt.spawn(async move {
            let r = net::Net::join(ep, id, name, pw).await.map(|c| (c, None));
            let _ = tx.send(r);
        });
        self.pending = Some(rx);
        self.goto(Page::Connecting("TUNING IN".into(), label.to_uppercase()));
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

    fn leave(&mut self, why: Option<(&str, &str)>) {
        self.conn = None;
        self.shutdown_hosting();
        self.game_id = None;
        let t = self.t();
        self.mode = if self.calm || self.session.is_none() { Mode::Room } else { Mode::PowerOff(t) };
        self.sfx.play(Sound::Back);
        match why {
            Some(w) => self.fail(w),
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
                (self.sel_t, self.sel_dir) = (self.t(), -1);
                self.sfx.play(Sound::Tick);
            }
            KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('j') | KeyCode::Tab => {
                self.sel = (self.sel + 1) % n;
                (self.sel_t, self.sel_dir) = (self.t(), 1);
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
                Page::Connecting(..) => {
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
            let mut cv = Canvas::for_area(a, pal::STAGE_DEEP);
            let mut lines: Vec<(String, Rgb)> = wrap_label("PartyLine needs a window of at least 80 x 24.", cv.w - 6).into_iter().map(|l| (l, pal::INK)).collect();
            lines.extend(wrap_label(&format!("Yours is {} x {}.", a.width, a.height), cv.w - 6).into_iter().map(|l| (l, pal::DIM)));
            let y0 = cv.h / 2 - lines.len() as i32 * LINE / 2;
            for (i, (l, c)) in lines.iter().enumerate() {
                cv.label_center(cv.w / 2, y0 + i as i32 * LINE, l, *c, 1);
            }
            cv.blit(buf, a);
            return;
        }
        let t = self.t();
        let rest = room::rest(a, MENU_W, MENU_H);
        match self.mode {
            Mode::Room => self.draw_room(buf, a, rest, true),
            Mode::ZoomIn(s) => self.draw_room(buf, a, rest + (1.0 - rest) * ease_in_out(prog(t, s, ZOOM_IN)), false),
            Mode::ZoomOut(s) => {
                let p = ease_in_out(prog(t, s, ZOOM_OUT));
                self.draw_room(buf, a, 1.0 - (1.0 - rest) * p, p > 0.9);
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

    /// The room, `p` of the way along the dolly into the TV; `menu` puts the TV menu on the glass.
    fn draw_room(&self, buf: &mut Buffer, a: Rect, p: f32, menu: bool) {
        let t = self.t();
        let mut shot = room::draw(a, p, t, self.glow);
        if menu {
            let status = self.menu(&mut shot.cv, shot.glass, t);
            self.notice(&mut shot.cv, shot.glass.1, status);
        }
        shot.cv.blit(buf, a);
    }

    /// Lines on the wall above the TV: news (a new version waiting, the network's state), then
    /// the page's status when the glass had no room for it.
    fn notice(&self, cv: &mut Canvas, glass_top: i32, status: Option<String>) {
        let news = if let Some(tag) = &self.updated {
            Some((format!("{tag} installed · restart to play"), pal::GOLD))
        } else if self.net_err.is_some() {
            Some(("no network".to_string(), pal::DIM))
        } else if self.net.is_none() {
            Some(("warming up the network…".to_string(), pal::DIM))
        } else {
            None
        };
        let lines: Vec<(String, Rgb)> = news.into_iter().chain(status.map(|s| (s, pal::INK))).collect();
        // Centered on the wall, clear of the cabinet's top edge when there is room for that.
        let wall = if glass_top - 6 >= LINE { glass_top - 6 } else { glass_top - 1 };
        let fit = (wall / LINE).max(0) as usize;
        let shown = &lines[lines.len().saturating_sub(fit)..];
        let mut y = (wall - shown.len() as i32 * LINE) / 2 + 1;
        for (s, c) in shown {
            cv.label_center(cv.w / 2, y, &fit_label(s, cv.w - 8, 1), *c, 1);
            y += LINE;
        }
    }

    /// The TV menu on the glass `g` (x0, y0, x1, y1 in canvas pixels, inclusive). Top to
    /// bottom: the logo (or a small title), the channel wheel (the channel on air big in the
    /// display font under its caption, its neighbours dim above and below), key hints. Parts
    /// give way, least useful first, until it fits the tube.
    /// Returns the page's status line when the glass had no room for it.
    fn menu(&self, cv: &mut Canvas, g: (i32, i32, i32, i32), t: f32) -> Option<String> {
        let (gx, gy, gw, gh) = (g.0, g.1, g.2 - g.0 + 1, g.3 - g.1 + 1);
        if gw < 24 || gh < 20 {
            return self.status();
        }
        if t - self.chan_t < 0.14 {
            snow(cv, g, 0.85, (t * 120.0) as u32);
            return None;
        }
        let (cx, maxw) = (gx + gw / 2, gw - 8);
        let (connecting, error) = (matches!(self.page, Page::Connecting(..)), matches!(self.page, Page::Error(..)));
        if connecting || error {
            snow(cv, g, if error { 0.16 } else { 0.1 }, (t * 30.0) as u32);
        }
        let items = self.items();
        let n = items.len();
        let wheel = n > 1;

        // What is on air: the headline, its caption, and for errors what to do about it.
        let typed = self.edit.as_ref().map(|(f, s)| if matches!(f, Field::Password | Field::JoinPw) { "*".repeat(s.chars().count()) } else { s.to_uppercase() });
        let (head, placeholder, extra) = match &self.page {
            Page::Connecting(h, _) | Page::Error(h, _) => (h.clone(), false, None),
            _ => match items.get(self.sel) {
                Some((name, it)) => {
                    let (h, p, e) = self.channel(*it, name);
                    (typed.clone().unwrap_or(h), p && typed.is_none(), e)
                }
                None => (String::new(), false, None),
            },
        };
        let mut caption: Vec<(String, Rgb)> = match &self.page {
            Page::Connecting(_, what) => vec![(what.clone(), pal::DIM)],
            Page::Error(..) => vec![],
            // A setting is captioned by what it is; anything else by its channel number, in TV
            // green, flaring when it changes.
            _ => match extra {
                Some(e) => vec![(e, pal::INK.lerp(pal::DIM, 0.4))],
                None => vec![(format!("CH {:02}", self.sel + 1), pal::GREEN.lerp(pal::INK, 1.0 - prog(t, self.sel_t, 0.45)))],
            },
        };
        if let Some((last, rest)) = caption.split_last_mut() {
            let others = label_width(&rest.iter().map(|c| c.0.as_str()).collect::<String>(), 1);
            last.0 = fit_label(&last.0, gw - 4 - others, 1);
        }
        let body = match &self.page {
            Page::Error(_, why) => wrap_label(why, maxw - 2),
            _ => vec![],
        };
        let status = self.status();

        // Fit: the first (logo scale, neighbours, status) that leaves room for the headline at
        // 7 px caps or more wins, at the largest size that fits. One size for every channel on
        // the page, so it doesn't jump while flipping; a channel too long for that gets its own.
        let line = SMALL + 1;
        let header = |logo: i32, st: bool| match logo {
            -1 => 0,
            0 => line + 1 + if st && status.is_some() { line + 1 } else { 0 } + 1,
            l => type_h((7 * l) as f32) + 2,
        };
        let middle = |nb: bool, head_h: i32| {
            (if nb { 2 * (line + 2) } else { 0 })
                + if caption.is_empty() { 0 } else { line + 1 }
                + head_h
                + if body.is_empty() { 0 } else { 2 + body.len() as i32 * LINE - 1 }
        };
        let keys_h = SMALL + 3;
        // Margins of 1 px at the top and bottom of the tube, 2 px above the key hints.
        let fits_h = |logo: i32, nb: bool, st: bool, head_h: i32| header(logo, st) + middle(nb, head_h) + keys_h + 4 <= gh;
        let logos: Vec<i32> = if self.branded() { [2, 1].into_iter().filter(|&l| big_width("PARTYLINE", l) <= maxw).collect() } else { vec![] };
        let nbs = if wheel { vec![true, false] } else { vec![false] };
        let mut configs: Vec<(i32, bool, bool)> = vec![];
        for &nb in &nbs {
            configs.extend(logos.iter().map(|&l| (l, nb, false)));
        }
        configs.extend(nbs.iter().map(|&nb| (0, nb, true)));
        configs.extend([(0, false, false), (-1, false, false)]);
        let heads: Vec<String> = if wheel { items.iter().map(|(name, it)| self.channel(*it, name).0).collect() } else { vec![head.clone()] };
        let pick = |hs: &[String]| {
            configs.iter().find_map(|&(l, nb, st)| {
                let top = if l > 0 { (7 * l) as f32 } else { 12.0 };
                [12.0f32, 10.0, 9.0, 8.0, 7.0]
                    .into_iter()
                    .find(|&c| c <= top && fits_h(l, nb, st, type_h(c)) && hs.iter().all(|h| type_width(h, c) <= maxw))
                    .map(|c| (l, nb, st, c))
            })
        };
        let (logo, nb, st, cap) = pick(&heads).or_else(|| pick(std::slice::from_ref(&head))).unwrap_or((-1, false, false, 0.0));
        let head_h = if cap > 0.0 { type_h(cap) } else { line };
        let st = st && logo == 0;

        // Header.
        let mut y = gy + 1;
        let head_w = if logo > 0 {
            let (s, w) = (logo, big_width("PARTYLINE", logo));
            cv.big_center(cx, y, "PARTYLINE", s, &|x, yy| {
                let sweep = ((t * 0.4).fract() * (w + 30) as f32) as i32 - 15;
                let c = pal::INK.lerp(pal::GOLD, yy as f32 / (7 * s) as f32);
                if (x + yy / 2 - sweep).abs() < 2 { c.lerp(pal::INK, 0.7) } else { c }
            });
            w
        } else if logo == 0 {
            let title = if self.branded() { "PARTYLINE" } else { self.page_title() };
            cv.heading(cx, y + 1, title, pal::GOLD, maxw);
            if let (true, Some(s)) = (st, &status) {
                cv.label_center(cx, y + 1 + line + 1, &fit_label(s, maxw, 1), pal::DIM, 1);
            }
            gfx::label_width(title, 2)
        } else {
            0
        };
        y += header(logo, st);
        // The tape light, in the corner when the header leaves room for it.
        let dot = gx + gw - 9;
        if logo >= 0 && dot > cx + head_w / 2 + 3 && (t * 1.4).fract() < 0.6 {
            cv.label(dot, gy + 2, "●", pal::RED, 1);
        }

        // The wheel, centered between the header and the key hints.
        let keys_y = gy + gh - 1 - keys_h;
        let mut y = y + ((keys_y - 2 - y - middle(nb, head_h)) / 2).max(0);
        let dim = pal::DIM.mul(0.85);
        if nb {
            if n > 2 {
                cv.label_center(cx, y, &fit_label(&items[(self.sel + n - 1) % n].0, maxw, 1), dim, 1);
            }
            y += line + 2;
        }
        if !caption.is_empty() {
            let w = label_width(&caption.iter().map(|c| c.0.as_str()).collect::<String>(), 1);
            let mut x = cx - w / 2;
            for (s, c) in &caption {
                x += cv.label(x, y, s, *c, 1) + 1;
            }
            y += line + 1;
        }
        // The channel rolls in from the side it came from and brightens.
        let roll = ease_out(prog(t, self.sel_t, 0.2));
        let hy = y + ((1.0 - roll) * 4.0).round() as i32 * self.sel_dir;
        let lit = if connecting { 0.8 + 0.2 * (t * 5.0).sin() } else { 0.4 + 0.6 * roll };
        let (c0, c1) = if error {
            (pal::ORANGE, pal::RED)
        } else if placeholder {
            (pal::DIM, pal::DIM)
        } else {
            (pal::INK, pal::GOLD)
        };
        cv.light(cx as f32, (hy + head_h / 2) as f32, gw as f32 * 0.4, self.glow, 0.1);
        // Typing can outgrow the size: smaller, then the small face, then just its end.
        let (end, top, h) = match [12.0f32, 10.0, 9.0, 8.0, 7.0].into_iter().find(|&c| c <= cap && type_width(&head, c) <= maxw) {
            Some(c) => {
                let top = hy + (head_h - type_h(c)) / 2;
                cv.type_center(cx, top, &head, c, &|_, yy| c0.lerp(c1, yy as f32 / c).mul(lit));
                (cx + type_width(&head, c) / 2, top, c.round() as i32)
            }
            None => {
                let s = if typed.is_some() { tail_label(&head, maxw - 3) } else { fit_label(&head, maxw, 1) };
                let top = hy + (head_h - line) / 2;
                cv.label_center(cx, top, &s, c0.mul(lit), 1);
                (cx + label_width(&s, 1) / 2, top, SMALL)
            }
        };
        if typed.is_some() && (t * 2.0).fract() < 0.5 {
            cv.rect(end + 2, top, 1, h + 1, pal::INK);
        }
        y += head_h;
        if !body.is_empty() {
            y += 2;
            for l in &body {
                cv.label_center(cx, y, l, pal::INK, 1);
                y += LINE;
            }
            y -= 1;
        }
        if nb {
            cv.label_center(cx, y + 2, &fit_label(&items[(self.sel + 1) % n].0, maxw, 1), dim, 1);
        }

        // Key hints.
        let mut hints: Vec<Hint> = vec![];
        if self.edit.is_some() {
            hints.extend([("ENTER", "save"), ("ESC", "cancel")]);
        } else if connecting {
            hints.push(("ESC", "cancel"));
        } else if error {
            hints.push(("ENTER", "ok"));
        } else {
            if items.get(self.sel).map(|i| i.1) == Some(Item::Bots) {
                hints.push(("←→", "bots"));
            }
            hints.extend([("↑↓", "channel"), ("ENTER", "select")]);
            if self.page != Page::Main {
                hints.push(("ESC", "back"));
            }
        }
        cv.keys(cx, keys_y, gw - 6, &hints, t);
        if st { None } else { status }
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
                studio::hud(buf, a, s, g.kind(), g.status(now));
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
            Phase::Results { gains, until, .. } => studio::results(buf, a, s, gains, *until, now, t, self.phase_since),
            Phase::Final => studio::finale(buf, a, s, host, t, self.phase_since, &mut self.local),
        }
        if let Some((msg, t0)) = &self.toast {
            let k = t - t0;
            if k < 1.6 {
                let mut cv = Canvas::from_buf(buf, a);
                let y = cv.h - 22 + if k < 0.2 { ((0.2 - k) * 20.0) as i32 } else { 0 };
                cv.chip_center(cv.w / 2, y, msg, pal::STAGE_DEEP, pal::GOLD);
                cv.blit(buf, a);
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

/// The screen under the score bar.
fn game_area(a: Rect) -> Rect {
    Rect::new(a.x, a.y + studio::HUD_ROWS, a.width, a.height.saturating_sub(studio::HUD_ROWS))
}

/// TV snow over the glass `g` (x0, y0, x1, y1, inclusive), `a` strong, fading into the tube's
/// corners.
fn snow(cv: &mut Canvas, g: (i32, i32, i32, i32), a: f32, seed: u32) {
    let (w, h) = ((g.2 - g.0 + 1) as f32, (g.3 - g.1 + 1) as f32);
    for y in g.1..=g.3 {
        for x in g.0..=g.2 {
            let (nx, ny) = ((x - g.0) as f32 / w * 2.0 - 1.0, (y - g.1) as f32 / h * 2.0 - 1.0);
            let fade = 1.0 - (nx.abs().powi(6) + ny.abs().powi(6)).min(1.0);
            let v = (hash(seed ^ (x as u32).wrapping_mul(31), y as u32) % 100) as f32 / 100.0;
            cv.blend(x, y, Rgb(v, v, v * 1.05), a * fade);
        }
    }
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
    match (code.get(..6), code.get(code.len().saturating_sub(4)..)) {
        (Some(a), Some(b)) if code.len() > 12 => format!("{a}...{b}"),
        _ => code.to_string(),
    }
}

/// Turn a failed connection into a headline and a short line a player can act on.
fn friendly(e: &anyhow::Error) -> (&'static str, &'static str) {
    let raw = e.to_string();
    if raw.contains("Wrong password") {
        ("WRONG PASSWORD", "ask the host for it")
    } else if raw.contains("full") {
        ("ROOM FULL", "eight players is the limit")
    } else if raw.contains("Timed out") {
        ("NO ANSWER", "the room took too long")
    } else if raw.contains("PartyLine v") {
        ("WRONG VERSION", "restart PartyLine to update")
    } else {
        ("NO SIGNAL", "is the host still in the lobby?")
    }
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
        let mut app = App::new(rt.handle().clone(), Args { name: "LUAN".into(), bots: 3, calm: false, mute: true, update: false });
        let dir = std::path::Path::new("target/snap");
        std::fs::create_dir_all(dir).unwrap();
        let save = |name: &str, buf: &Buffer| std::fs::write(dir.join(format!("{name}.png")), png(buf)).unwrap();

        app.net_err = Some("test".into());
        for (w, h) in [(200u16, 50u16), (120, 30), (80, 24)] {
            let a = Rect::new(0, 0, w, h);
            let tag = format!("{w}x{h}");
            let rest = room::rest(a, MENU_W, MENU_H);
            app.chan_t = -10.0;
            for (name, page, glow) in [
                ("room", Page::Main, TV_COLD.lerp(pal::GOLD, 0.6)),
                ("room_host", Page::Host, TV_COLD.lerp(pal::GOLD, 0.6)),
                ("room_join", Page::Join, TV_COLD.lerp(hue(1), 0.6)),
                ("room_error", Page::Error("NO SIGNAL".into(), "is the host still in the lobby?".into()), TV_COLD.lerp(pal::RED, 0.6)),
                ("room_tuning", Page::Connecting("TUNING IN".into(), "LUAN'S ROOM".into()), TV_COLD),
            ] {
                let mut buf = Buffer::empty(a);
                (app.page, app.glow, app.sel) = (page, glow, 0);
                app.draw_room(&mut buf, a, rest, true);
                save(&format!("{name}_{tag}"), &buf);
            }
            app.page = Page::Main;
            app.sel = 2;
            app.edit = Some((Field::Name, "Luanzinho".into()));
            let mut buf = Buffer::empty(a);
            app.draw_room(&mut buf, a, rest, true);
            save(&format!("room_edit_{tag}"), &buf);
            (app.edit, app.sel) = (None, 0);
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
                studio::hud(b, a, s, Kind::Blink, ("FLASH 2 OF 3".into(), pal::DIM));
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
                studio::hud(b, a, s, Kind::Gridlock, ("3 OF 4 MOVING".into(), pal::DIM));
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
                studio::hud(b, a, s, Kind::Derby, ("42 S LEFT".into(), pal::DIM));
            });

            let mut g = odd::S::new(&ids, 3, 0);
            for (p, k) in [(1u8, 0u8), (2, 0), (3, 2), (4, 1)] {
                g.input(p, &Input::Pick(k), &mut ev);
            }
            g.tick(1, &mut ev);
            let mut l3 = Local { reveal_t: 0.0, ..Local::default() };
            shot("odd", &s, &mut |b, s| {
                g.render(&v(a, &s.players), &mut l3, b);
                studio::hud(b, a, s, Kind::Odd, ("QUESTION 2 OF 5".into(), pal::DIM));
            });

            let gains = vec![(1, 4), (2, 2), (4, 1), (3, 0)];
            shot("results", &s, &mut |b, s| studio::results(b, a, s, &gains, 4000, 0, 10.0, 0.0));
            s.phase = Phase::Final;
            let mut l4 = Local::default();
            shot("final", &s, &mut |b, s| studio::finale(b, a, s, true, 10.0, 0.0, &mut l4));
            let mut buf = Buffer::empty(a);
            studio::lobby(&mut buf, a, &s, 1, true, 10.0, &[]);
            studio::go(&mut buf, a, 0.3);
            save(&format!("go_{tag}"), &buf);
        }
        // One page with every screen, for a single scroll-through review.
        let mut names: Vec<_> = std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().into_string().unwrap()).filter(|n| n.ends_with(".png")).collect();
        names.sort();
        let mut g = String::from("<!doctype html><meta charset=utf-8><body style='background:#111;color:#aaa;font:12px sans-serif'>");
        for n in names {
            g.push_str(&format!("<h3>{n}</h3><img src='{n}' style='image-rendering:pixelated;max-width:100%'>"));
        }
        std::fs::write(dir.join("gallery.html"), g).unwrap();
    }

    /// The frame as it looks: every cell is two pixels (a half block), any other glyph would
    /// show as a red cell so it stands out.
    fn png(buf: &Buffer) -> Vec<u8> {
        const K: usize = 4;
        let (w, h) = (buf.area.width as usize, buf.area.height as usize * 2);
        let mut px = vec![0u8; w * K * h * K * 3];
        for y in 0..h {
            for x in 0..w {
                let c = &buf[(x as u16, (y / 2) as u16)];
                let half = c.symbol() == "▀";
                let rgb = match (half, y % 2) {
                    (true, 0) => Rgb::from_color(c.fg),
                    (_, _) if !half && c.symbol() != " " => Rgb(1.0, 0.0, 0.0),
                    _ => Rgb::from_color(c.bg),
                };
                let v = [rgb.0, rgb.1, rgb.2].map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8);
                for dy in 0..K {
                    for dx in 0..K {
                        let i = ((y * K + dy) * w * K + x * K + dx) * 3;
                        px[i..i + 3].copy_from_slice(&v);
                    }
                }
            }
        }
        let mut out = vec![];
        let mut e = png::Encoder::new(&mut out, (w * K) as u32, (h * K) as u32);
        e.set_color(png::ColorType::Rgb);
        e.write_header().unwrap().write_image_data(&px).unwrap();
        out
    }

}
