# Design

Recorded from the built app (`src/gfx.rs`, `src/room.rs`, `src/studio.rs`). Surface brief:
`.impeccable/surfaces/src-main-rs.md`.

## World

A live TV game show you watch from your couch, then get pulled into.

- **Living room (menu):** night scene in half-block pixel art. The CRT TV is the menu; its
  glow tints the walls, couch and the two friends' heads with the color of the highlighted item.
- **Studio (everything after you dive in):** royal stage blue, sweeping spotlights, gold
  marquee bulbs around the frame, red contestant podiums.

## Color

All colors are truecolor constants in `gfx::pal`.

| Role | Token | Hex |
|---|---|---|
| Stage ground | `STAGE` / `STAGE_DEEP` | `#2235B0` / `#0A0F45` |
| Marquee bulbs, titles | `BULB`, `GOLD`, `ORANGE` | `#FFD25A`, `#FFC23A`, `#FF8A2A` |
| Podiums | `PODIUM` / `PODIUM_DARK` | `#C8243C` / `#7E1428` |
| Text | `INK` / `DIM` | `#F6F1E4` / `#98A2E6` (dim text is tinted from the stage, never grey) |
| Status | `GREEN`, `RED` | `#4BE38A`, `#FF4058` |
| Room | `WALL_*`, `FLOOR`, `LAMP`, `COUCH`, `TV_BODY` | indigo night, warm wood, amber lamp |

Each player owns one of 8 saturated hues (`gfx::hue`) everywhere: podium cap, trail, racer,
score chip. An eliminated player's hue drains toward grey (`Rgb::grey`).

## Type

- Display: hand-authored 5x7 pixel font (`gfx::glyph`) at integer scales, gold gradient
  with a moving shimmer and a short drop shadow.
- Body: the terminal's own font, sentence case for guidance, caps for labels.

## Motion

Custom curves only (`gfx::ease_out` = cubic-bezier(0.23, 1, 0.32, 1),
`gfx::ease_in_out` = cubic-bezier(0.77, 0, 0.175, 1)).

| Moment | Motion |
|---|---|
| Select in the TV menu | camera dollies into the screen, 700 ms ease-in-out |
| Studio appears | CRT power-on line opens, 260 ms ease-out |
| Leave | CRT collapses to a line then a dot (320 ms), camera pulls back (550 ms) |
| Menu cursor | instant, a tick sound only (used constantly, so no animation) |
| Page change | 140 ms channel static |
| Podiums | rise in with 60 ms stagger |
| Title cards | typewriter, 35 ms per letter |
| Countdown | digit pops one scale larger for 120 ms, bulbs blaze |
| GO / NOW / crash | grid-breaking giant type, screen shake, flash tint |
| Results | rows slide in with 90 ms stagger, totals count up |
| Marquee | linear chase (constant motion) |

`--calm` removes shake and camera moves and softens flashes.

## Sound

Procedural synth (`src/sfx.rs`), no asset files: ticks, select, channel static, whoosh,
countdown beeps, GO chord, crash, typo, fanfare.
