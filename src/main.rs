//! PARTYLINE: a terminal party game show. Host a room, friends join over LAN or internet
//! (no server to run), play a run of minigames, crown a champion.
mod app;
mod art;
mod games;
mod gfx;
mod net;
mod proto;
mod room;
mod server;
mod sfx;
mod studio;
mod update;

use std::io::stdout;
use std::time::Duration;

use ratatui::crossterm::{
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
};

const HELP: &str = "partyline [--name NAME] [--bots N] [--calm] [--mute] [--no-update]

  --name NAME   your name on the podium (default: your login name)
  --bots N      bots that join rooms you host (0-7, default 0)
  --calm        no screen shake, flashes softened, camera cuts instead of moves
  --mute        no sound
  --no-update   don't check GitHub for a newer version";

fn main() -> anyhow::Result<()> {
    let mut args = app::Args { name: default_name(), bots: 0, calm: false, mute: false, update: true };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--name" => args.name = it.next().unwrap_or(args.name),
            "--bots" => args.bots = it.next().and_then(|n| n.parse().ok()).unwrap_or(0).min(7),
            "--calm" => args.calm = true,
            "--mute" => args.mute = true,
            "--no-update" => args.update = false,
            _ => {
                println!("PARTYLINE v{}\n\n{HELP}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
        }
    }

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let _guard = rt.enter();
    let mut terminal = ratatui::init();
    let _ = execute!(stdout(), EnableBracketedPaste);
    let mut app = app::App::new(rt.handle().clone(), args);
    let result = app.run(&mut terminal);
    let _ = execute!(stdout(), DisableBracketedPaste);
    ratatui::restore();
    drop(app);
    rt.shutdown_timeout(Duration::from_millis(300));
    result
}

fn default_name() -> String {
    let raw = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "PLAYER".into());
    let n: String = raw.chars().filter(|c| c.is_ascii_alphanumeric()).take(12).collect();
    if n.is_empty() { "PLAYER".into() } else { n }
}
