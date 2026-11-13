# Design

Recorded from the built app (`src/gfx.rs`, `src/room.rs`, `src/studio.rs`) and the art
pipeline (`tools/scene.py`, `tools/bake.py`). Surface brief: `.impeccable/surfaces/src-main-rs.md`.

## World

A grimy basement where the crew watches an old CRT, and the test chamber inside that TV.
Low-poly 3D, PS1-era grain, one bare bulb. Grim and funny, never cute.

- **Living room (menu):** pre-rendered in Blender. Behind a worn couch, four contestants in
  improvised hats (traffic cone, welding mask, beanie, top hat) face a wood-panel console CRT.
  The menu lives on the TV glass. The TV's light on the room takes the color of the highlighted
  item; the bulb buzzes and browns out now and then.
- **Dolly:** picking a room flies the camera from the couch, between the two heads, into the
  glass (20 rendered frames, crossfaded). The CRT then powers on.
- **Test chamber (everything inside the TV):** stained concrete wall with "SMILE. YOU ARE ON TV."
  painted on it, pipes, crates, a hanging bulb. Seen through CRT glass: rounded tube corners,
  vignette, scanlines, a slow rolling bar.

## Art pipeline

`python tools/bake.py` runs `tools/scene.py` in Blender (Cycles), then packs `assets/`:

| Asset | What |
|---|---|
| `room_a.png` | dolly frames lit by the bulb only, stacked vertically, 320x180 each |
| `room_b.png` | same frames lit by the TV only (white); alpha = where the glass is visible |
| `crew.png` | the 8 contestants seated on chairs (index = player hue), then the empty chair |
| `chamber.png` | the test chamber backdrop |
| `font.png`, `font.txt` | Bahnschrift Bold Condensed glyph atlas, chipped and scratched |

The game embeds these and composites them live: `room = bulb_pass * flicker + tv_pass * glow`,
the glass drawn where the mask says, then a film grade and a 6-bit ordered dither.

## Color

All colors are truecolor constants in `gfx::pal`.

| Role | Token | Hex |
|---|---|---|
| Shadow, panels | `STAGE_DEEP` / `PANEL` / `BLACK` | `#121210` / `#1C1B16` / `#080807` |
| Stencil paint, titles | `INK` → `GOLD` | `#E8E0C8` → `#E2A42C` |
| Rust, walls | `ORANGE`, `PODIUM`, `PODIUM_DARK` | `#C7562A`, `#7A2A1E`, `#3E1810` |
| Crates | `WOOD` | `#5E4329` |
| Bulb | `BULB` | `#FFC56A` |
| Text | `INK` / `DIM` | `#E8E0C8` / `#8F8A74` |
| Status | `GREEN`, `RED` | `#8CC24A`, `#D0352B` |
| Idle CRT light | `app::TV_COLD` | `#A9C4D6` |

Each player owns one of 8 hues (`gfx::hue`), the same list the contestant jackets are rendered
in. An eliminated player's hue drains toward grey (`Rgb::grey`).

## Type

- Display: Bahnschrift Bold Condensed, distressed, area-sampled from a 56 px cap atlas to any
  size (`Canvas::big`, caps are `7 * scale` pixels). Bone-to-amber gradient with a moving shimmer.
- Body: the terminal's own font, sentence case for guidance, caps for labels.

## Motion

Custom curves only (`gfx::ease_out` = cubic-bezier(0.23, 1, 0.32, 1),
`gfx::ease_in_out` = cubic-bezier(0.77, 0, 0.175, 1)).

| Moment | Motion |
|---|---|
| Select in the TV menu | camera dollies into the glass, 700 ms ease-in-out |
| Inside appears | CRT power-on line opens, 260 ms ease-out |
| Leave | CRT collapses to a line then a dot (320 ms), camera pulls back (550 ms) |
| Menu cursor | instant, the TV light in the room shifts color |
| Page change | 140 ms channel static |
| Contestants | drop into their chairs, 400 ms ease-out, 60 ms stagger |
| Title cards | typewriter, 35 ms per letter |
| Countdown | digit pops one scale larger for 120 ms, bulb flares |
| GO / NOW / crash | grid-breaking giant type, screen shake, flash tint |
| Results | rows slide in with 90 ms stagger, totals count up |
| Finale | crates rise (3rd, 2nd, champion), each contestant drops onto theirs |
| Bulb | constant buzz, random brownout |

`--calm` removes shake and camera moves and softens flashes.

## Sound

Procedural synth (`src/sfx.rs`), no asset files: ticks, select, channel static, whoosh,
countdown beeps, GO chord, crash, typo, fanfare.
