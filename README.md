# PARTYLINE

A party game show that runs in your terminal. One person hosts, friends join over the LAN
or the internet (no server to run, no port forwarding), everyone plays a run of minigames,
and a champion gets crowned on a crate in a grimy basement.

## Play

Nothing to install first. Paste one line into a terminal; it downloads the game and starts it.

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/luannzin/partyline/main/install.ps1 | iex
```

Linux (x86_64) and macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/luannzin/partyline/main/install.sh | sh
```

After that, just run `partyline`. The Windows script puts it in
`%LOCALAPPDATA%\Programs\PartyLine` and adds that to your PATH; the Linux/macOS one puts it in
`~/.local/bin`. Or grab the file for your OS from
[Releases](https://github.com/luannzin/partyline/releases) and run it (on Windows: **More info →
Run anyway** the first time, the build isn't signed).

PartyLine updates itself: on start it checks for a newer release, swaps it in, and the TV says
so; the next launch is the new version. Everyone in a room must run the same version, so if a
friend can't join, both of you restart once. `--no-update` turns this off.

Flags: `--name NAME`, `--bots N` (bots fill rooms you host), `--calm` (no shake, camera
cuts instead of moves), `--mute`, `--no-update`.

Use Windows Terminal (or any truecolor terminal) at 80x24 or bigger; bigger looks better. The
first time you host, Windows Firewall asks to allow the game: allow it, or LAN players can't
find you. Linux needs ALSA for sound (`libasound2`, already on desktop installs).

## Run from source

```bash
cargo run --release -- --bots 3
```

## Playing with friends

- **Same network:** host a room; it appears in everyone's JOIN A ROOM list on its own.
- **Anywhere else:** in the lobby press `C` to copy the invite code and send it. Friends pick
  JOIN A ROOM, then ENTER AN INVITE CODE, and paste it.
- A password on the room works like a Radmin network password.
- The lobby shows how each player is connected: `LAN`, `NET` (direct over the internet)
  or `RELAY` (when no direct path exists, traffic goes through an encrypted relay).

## Controls

| Where | Keys |
|---|---|
| TV menu | `↑↓` choose, `Enter` select, `Esc` back |
| Lobby | `Space` ready, `S` sit out and watch, `C` copy invite code, `Enter` start (host), `←→` rounds (host), `Esc` leave |
| Blink | `Space` the instant the screen flashes |
| Gridlock | arrows or `WASD` |
| Keyboard Derby | type the word |
| Odd One Out | `1` `2` `3` |
| Watching | `Tab` / `←→` switch camera |

## How it works

- `src/net.rs`: [iroh](https://github.com/n0-computer/iroh) QUIC connections dialed by public
  key (hole punching, relay fallback) and mDNS room discovery. That's the built-in "virtual LAN".
- `src/server.rs`: the host runs the session; every client gets the full state about 30 times a
  second and only sends key presses.
- `src/games/`: each minigame owns its rules, its bot, and its rendering.
- `src/room.rs`, `src/studio.rs`: the living room with the TV, and the test chamber inside it.
- `src/art.rs`: the pre-rendered art in `assets/`, embedded in the binary.
- `src/update.rs`: self-update from GitHub Releases.

## Releases

Bump `version` in `Cargo.toml` and push to `main`. GitHub Actions sees there's no release for
that version yet, builds Windows, Linux and macOS (universal) binaries and publishes release
`v<version>`; installed copies pick it up on their next start. No manual tagging.

## Art

The room, the camera move into the TV, the contestants, the chamber and the display font are
rendered in Blender and baked by a Python script. The game only needs the baked `assets/`;
rebuild them after changing `tools/scene.py`:

```bash
python tools/bake.py
```

Needs Blender 5.x (set `BLENDER` if it isn't in the default install path), Pillow and numpy.
`--quick` renders a fast low-quality pass for iterating, `--pack` only repacks.

## Tests

```bash
cargo test
```

```bash
cargo test -- --ignored
```

The ignored tests open two real network endpoints on this machine and render every screen
to `target/snap/*.html` for visual review.
