"""Bake PARTYLINE art into assets/ (embedded into the binary at compile time).

    python tools/bake.py              render with Blender, then pack
    python tools/bake.py --quick      fast low-sample render (3 dolly frames) for iterating
    python tools/bake.py --pack       skip Blender, repack what target/raw already has

Needs Blender 5.x (set BLENDER if it is not in the default place) and Pillow + numpy.
"""
import os
import pathlib
import subprocess
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont

ROOT = pathlib.Path(__file__).resolve().parent.parent
RAW = ROOT / "target" / "raw"
OUT = ROOT / "assets"
BLENDER = os.environ.get("BLENDER", r"C:\Program Files\Blender Foundation\Blender 5.1\blender.exe")

if "--pack" not in sys.argv:
    cmd = [BLENDER, "-b", "--factory-startup", "-P", str(ROOT / "tools" / "scene.py"), "--", str(RAW)]
    subprocess.run(cmd + (["--quick"] if "--quick" in sys.argv else []), check=True)
OUT.mkdir(exist_ok=True)


def stack(images, axis):
    w = sum(i.width for i in images) if axis == 0 else images[0].width
    h = images[0].height if axis == 0 else sum(i.height for i in images)
    sheet = Image.new(images[0].mode, (w, h))
    off = 0
    for i in images:
        sheet.paste(i, (off, 0) if axis == 0 else (0, off))
        off += i.width if axis == 0 else i.height
    return sheet


# Dolly frames, stacked vertically. room_b carries the visible-screen mask in its alpha.
n = len(list(RAW.glob("a*.png")))
stack([Image.open(RAW / f"a{k:02}.png").convert("RGB") for k in range(n)], 1).save(OUT / "room_a.png", optimize=True)
b = []
for k in range(n):
    r, g, bl = Image.open(RAW / f"b{k:02}.png").convert("RGB").split()
    b.append(Image.merge("RGBA", (r, g, bl, Image.open(RAW / f"m{k:02}.png").getchannel("A"))))
stack(b, 1).save(OUT / "room_b.png", optimize=True)

# Contestant sprites side by side (8 contestants, then the empty chair).
stack([Image.open(RAW / f"crew{i}.png").convert("RGBA") for i in range(9)], 0).save(OUT / "crew.png", optimize=True)
Image.open(RAW / "chamber.png").convert("RGB").save(OUT / "chamber.png", optimize=True)

# Display font: Bahnschrift Bold Condensed, distressed like stencil paint. One row of glyph
# cells for ASCII 32..126; the game scales it down with area sampling.
CAP, PAD = 56, 4
font = ImageFont.truetype("C:/Windows/Fonts/bahnschrift.ttf", 80)
font.set_variation_by_name("Bold Condensed")
top, bottom = font.getbbox("H")[1], font.getbbox("H")[3]
font = ImageFont.truetype("C:/Windows/Fonts/bahnschrift.ttf", round(80 * CAP / (bottom - top)))
font.set_variation_by_name("Bold Condensed")
top = font.getbbox("H")[1]
H = CAP + CAP // 3
chars = [chr(c) for c in range(32, 127)]
adv = [round(font.getlength(c)) for c in chars]
atlas = Image.new("L", (sum(a + 2 * PAD for a in adv), H))
d = ImageDraw.Draw(atlas)
x = 0
for c, a in zip(chars, adv):
    d.text((x + PAD, -top), c, font=font, fill=255)
    x += a + 2 * PAD
rng = np.random.default_rng(7)
ink = np.asarray(atlas, dtype=np.float32)
wear = Image.new("L", atlas.size, 0)
wd = ImageDraw.Draw(wear)
for _ in range(atlas.width * H // 600):  # chipped paint
    cx, cy, r = rng.uniform(0, atlas.width), rng.uniform(0, H), rng.uniform(1, 3.5)
    wd.ellipse((cx - r, cy - r * 0.7, cx + r, cy + r * 0.7), fill=255)
for _ in range(atlas.width // 40):  # scratches
    y0 = rng.uniform(0, H)
    x0 = rng.uniform(0, atlas.width)
    wd.line((x0, y0, x0 + rng.uniform(20, 70), y0 + rng.uniform(-4, 4)), fill=255, width=1)
ink *= 1.0 - np.asarray(wear, dtype=np.float32) / 255.0 * 0.7
Image.fromarray(ink.clip(0, 255).astype(np.uint8)).save(OUT / "font.png", optimize=True)
(OUT / "font.txt").write_text(f"{CAP} {PAD}\n{' '.join(map(str, adv))}\n")

print("assets:", ", ".join(f"{p.name} {p.stat().st_size // 1024}K" for p in sorted(OUT.iterdir())))
