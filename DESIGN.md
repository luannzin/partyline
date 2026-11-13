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

Every word on screen is pixels drawn into the canvas; no terminal glyph is ever shown. Two
faces, all caps (accented letters fold to their base letter):

- Display: Bahnschrift Bold Condensed, distressed, area-sampled from a 56 px cap atlas
  (`Canvas::type_at` by cap height; `Canvas::big` is `7 * scale`). 7 px caps and up only:
  below that it turns to mush. Logo, titles, the TV menu's channel, champion, big numbers.
  Bone-to-amber gradient with a drop shadow; the logo adds a moving shimmer.
- Small: a crisp 5 px caps bitmap face with square DIN shapes to sit beside Bahnschrift
  (`gfx::glyph`), 1 px tracking, 1 px drop shadow, tabular digits, 7 px line pitch. Labels,
  hints, notes, names in tight places. Headings in it are spaced (2 px tracking, falling back
  to 1 when they don't fit: `Canvas::heading`). Wrapping is balanced (`wrap_label`); cuts end
  in an ellipsis (`fit_label`); a typed value keeps its end in view (`tail_label`); counts pick
  the longest wording that fits (`pick_label`).
- Plates: chips are the small face on a 7 px plate (`Canvas::chip`). Key hints are bone keycaps
  with a dim verb (`Canvas::keys`); when they don't all fit they take turns, a page every 3 s.
- TV menu: a channel wheel. The channel on air is set in the display font on the glass (7 to
  12 px caps, one size per page), captioned by its channel number in TV green or, for a
  setting, by what it is (the headline is then the value; typing happens in it). Neighbours
  sit dim above and below when the tube has room. Parts give way in order (neighbours, logo
  size, status line) until it fits; small windows also move the camera in along the dolly
  until the glass is at least 60 x 40 px (`room::rest`). News and page status the glass has no
  room for go on the wall above the TV.
- Lobby nameplates: your own lit in your hue, the others' names in their hue on a dark plate;
  nameplates wider than a seat alternate high and low. READY rides on the contestant's feet.
- Overlays drawn after a game (score bar, camera lower third, toast) read the frame back into
  pixels (`Canvas::from_buf`) and draw on that.

## Motion

Custom curves only (`gfx::ease_out` = cubic-bezier(0.23, 1, 0.32, 1),
`gfx::ease_in_out` = cubic-bezier(0.77, 0, 0.175, 1)).

| Moment | Motion |
|---|---|
| Select in the TV menu | camera dollies into the glass, 700 ms ease-in-out |
| Inside appears | CRT power-on line opens, 260 ms ease-out |
| Leave | CRT collapses to a line then a dot (320 ms), camera pulls back (550 ms) |
| Menu cursor | the channel rolls in from the side it came from (4 px, 200 ms ease-out) and brightens; `CH` flares; the TV light in the room shifts color |
| Tuning in / no signal | snow on the glass, the headline breathes |
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
