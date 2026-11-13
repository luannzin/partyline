//! Wire types shared by host and clients, plus length-prefixed postcard framing.
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::games::Game;

pub type Pid = u8;
pub const MAX_PLAYERS: usize = 8;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    Host,
    Bot,
    Lan,
    Direct,
    Relay,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Player {
    pub id: Pid,
    pub name: String,
    pub hue: u8,
    pub ready: bool,
    pub spectator: bool,
    pub score: u32,
    pub link: Link,
    pub ping: u16,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Blink,
    Gridlock,
    Derby,
    Odd,
}

pub const KINDS: [Kind; 4] = [Kind::Blink, Kind::Gridlock, Kind::Derby, Kind::Odd];

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Phase {
    Lobby,
    Intro { kind: Kind, go_at: u64 },
    Play(Game),
    Results { kind: Kind, gains: Vec<(Pid, u32)>, until: u64 },
    Final,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Session {
    pub room: String,
    pub code: String,
    pub players: Vec<Player>,
    pub phase: Phase,
    pub round: u8,
    pub rounds: u8,
}

impl Session {
    pub fn player(&self, id: Pid) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    pub fn delta(self) -> (i32, i32) {
        match self {
            Dir::Up => (0, -1),
            Dir::Down => (0, 1),
            Dir::Left => (-1, 0),
            Dir::Right => (1, 0),
        }
    }
    pub fn opposite(self) -> Dir {
        match self {
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
            Dir::Left => Dir::Right,
            Dir::Right => Dir::Left,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Dir(Dir),
    Early,
    React(u16),
    Word(u16),
    Pick(u8),
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Count(u8),
    Go,
    Flash,
    FalseStart(Pid),
    Reacted(Pid),
    Crash(Pid),
    Word(Pid),
    Finish(Pid),
    Locked(Pid),
    Reveal,
    Joined(Pid),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ClientMsg {
    /// `version` first, so any future layout change still reads it: host and guests must match.
    Hello { version: String, name: String, password: String },
    Ready(bool),
    Spectate(bool),
    Rounds(u8),
    Start,
    Input(Input),
    ToLobby,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ServerMsg {
    Welcome { you: Pid },
    Reject(String),
    Frame { now: u64, s: Box<Session>, ev: Vec<Event> },
}

pub async fn send<T: Serialize>(w: &mut iroh::endpoint::SendStream, m: &T) -> anyhow::Result<()> {
    let bytes = postcard::to_stdvec(m)?;
    w.write_all(&(bytes.len() as u32).to_le_bytes()).await?;
    w.write_all(&bytes).await?;
    Ok(())
}

pub async fn recv<T: DeserializeOwned>(r: &mut iroh::endpoint::RecvStream) -> anyhow::Result<T> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).await?;
    let n = u32::from_le_bytes(len) as usize;
    anyhow::ensure!(n <= 1 << 20, "frame too large: {n} bytes");
    let mut buf = vec![0; n];
    r.read_exact(&mut buf).await?;
    Ok(postcard::from_bytes(&buf)?)
}
