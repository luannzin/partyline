"""PARTYLINE art, built and rendered in Blender. Do not run directly: `python tools/bake.py`.

Renders into OUT (the argument after `--`):
  a00..aNN.png  living room lit by the hanging bulb only      (dolly frames, room shot -> inside the TV)
  b00..bNN.png  living room lit by the TV screen only (white)  (the game tints it at runtime)
  m00..mNN.png  alpha = where the TV screen is visible           (the game draws the menu there)
  crew0..7.png  each contestant seated on a chair, transparent   (lobby, finale)
  crew8.png     the empty chair
  chamber.png   the test chamber backdrop seen inside the TV
"""
import math
import os
import sys

import bpy

OUT = os.path.abspath(sys.argv[sys.argv.index("--") + 1])
QUICK = "--quick" in sys.argv
FRAMES = 3 if QUICK else 20
SAMPLES = 8 if QUICK else 64
os.makedirs(OUT, exist_ok=True)

# Player hues, same order as gfx::HUES in the game.
HUES = [0xD9533E, 0x4FA3C7, 0x8DB33A, 0xE39B2D, 0xC75B8E, 0x8C7BC9, 0x46B59A, 0xD8CB5A]
R = math.radians


def lin(h, k=1.0):
    def c(v):
        v /= 255
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4

    return (c(h >> 16 & 255) * k, c(h >> 8 & 255) * k, c(h & 255) * k, 1.0)


def desat(h, k):
    r, g, b = h >> 16 & 255, h >> 8 & 255, h & 255
    l = 0.3 * r + 0.59 * g + 0.11 * b
    mix = lambda v: int(v + (l - v) * k)
    return mix(r) << 16 | mix(g) << 8 | mix(b)


# ---------------- scene plumbing ----------------

bpy.ops.wm.read_factory_settings(use_empty=True)
sc = bpy.context.scene
sc.render.engine = "CYCLES"
sc.cycles.device = "CPU"
sc.cycles.max_bounces = 4
sc.cycles.use_denoising = True
sc.view_settings.view_transform = "Standard"
sc.render.image_settings.file_format = "PNG"
sc.render.image_settings.color_mode = "RGBA"
sc.render.resolution_percentage = 100
world = bpy.data.worlds.new("w")
sc.world = world
world.use_nodes = True
world.node_tree.nodes["Background"].inputs["Color"].default_value = (0.0, 0.0, 0.0, 1)


def mat(name, base, rough=0.8, dirt=0.5, scale=6.0, metal=0.0, stretch=(1, 1, 1), bump=0.25):
    """Grimy material: base color broken up by two octaves of noise, plus a little bump."""
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    b = nt.nodes["Principled BSDF"]
    b.inputs["Roughness"].default_value = rough
    b.inputs["Metallic"].default_value = metal
    coord = nt.nodes.new("ShaderNodeTexCoord")
    mp = nt.nodes.new("ShaderNodeMapping")
    mp.inputs["Scale"].default_value = stretch
    nt.links.new(coord.outputs["Object"], mp.inputs["Vector"])
    n1 = nt.nodes.new("ShaderNodeTexNoise")
    n1.inputs["Scale"].default_value = scale
    n1.inputs["Detail"].default_value = 8
    n2 = nt.nodes.new("ShaderNodeTexNoise")
    n2.inputs["Scale"].default_value = scale * 7
    n2.inputs["Detail"].default_value = 4
    for n in (n1, n2):
        nt.links.new(mp.outputs["Vector"], n.inputs["Vector"])
    mul = nt.nodes.new("ShaderNodeMath")
    mul.operation = "MULTIPLY"
    nt.links.new(n1.outputs["Fac"], mul.inputs[0])
    nt.links.new(n2.outputs["Fac"], mul.inputs[1])
    ramp = nt.nodes.new("ShaderNodeValToRGB")
    e = ramp.color_ramp.elements
    c = lin(base)
    e[0].position, e[0].color = 0.1, (c[0] * (1 - dirt), c[1] * (1 - dirt) * 0.95, c[2] * (1 - dirt) * 0.85, 1)
    e[1].position, e[1].color = 0.38, c
    nt.links.new(mul.outputs[0], ramp.inputs["Fac"])
    nt.links.new(ramp.outputs["Color"], b.inputs["Base Color"])
    if bump:
        bp = nt.nodes.new("ShaderNodeBump")
        bp.inputs["Strength"].default_value = bump
        nt.links.new(n2.outputs["Fac"], bp.inputs["Height"])
        nt.links.new(bp.outputs["Normal"], b.inputs["Normal"])
    return m


def plain(name, base, rough=0.5, metal=0.0, emit=None, strength=0.0):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    b = m.node_tree.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = lin(base)
    b.inputs["Roughness"].default_value = rough
    b.inputs["Metallic"].default_value = metal
    if emit is not None:
        b.inputs["Emission Color"].default_value = lin(emit)
        b.inputs["Emission Strength"].default_value = strength
    return m


def finish(o, m, parent, bevel, smooth=False):
    if bevel:
        mod = o.modifiers.new("bevel", "BEVEL")
        mod.width = bevel
        mod.segments = 2
    o.data.materials.append(m)
    if smooth:
        for p in o.data.polygons:
            p.use_smooth = True
    if parent:
        o.parent = parent
    return o


def box(size, loc, m, rot=(0, 0, 0), bevel=0.015, parent=None):
    bpy.ops.mesh.primitive_cube_add(size=1, location=loc, rotation=rot)
    o = bpy.context.active_object
    o.scale = size
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return finish(o, m, parent, bevel)


def cyl(r, depth, loc, m, rot=(0, 0, 0), verts=12, parent=None, r2=None):
    if r2 is None:
        bpy.ops.mesh.primitive_cylinder_add(vertices=verts, radius=r, depth=depth, location=loc, rotation=rot)
    else:
        bpy.ops.mesh.primitive_cone_add(vertices=verts, radius1=r, radius2=r2, depth=depth, location=loc, rotation=rot)
    return finish(bpy.context.active_object, m, parent, 0, smooth=True)


def ball(r, loc, m, scale=(1, 1, 1), parent=None):
    bpy.ops.mesh.primitive_uv_sphere_add(segments=12, ring_count=8, radius=r, location=loc)
    o = bpy.context.active_object
    o.scale = scale
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return finish(o, m, parent, 0, smooth=True)


def light(kind, loc, energy, color=(1, 1, 1), size=0.05, rot=(0, 0, 0)):
    d = bpy.data.lights.new(kind + "_light", kind)
    d.energy = energy
    d.color = color
    if kind == "AREA":
        d.shape = "RECTANGLE"
        d.size, d.size_y = size
    else:
        d.shadow_soft_size = size
    o = bpy.data.objects.new(d.name, d)
    o.location, o.rotation_euler = loc, rot
    sc.collection.objects.link(o)
    return o


def camera(loc, rot, lens=28, ortho=None):
    d = bpy.data.cameras.new("cam")
    d.lens, d.sensor_width, d.sensor_fit, d.clip_start = lens, 36, "HORIZONTAL", 0.03
    if ortho:
        d.type, d.ortho_scale = "ORTHO", ortho
    o = bpy.data.objects.new("cam", d)
    o.location, o.rotation_euler = loc, rot
    sc.collection.objects.link(o)
    sc.camera = o
    return o


def text(body, loc, size, m, rot=(R(90), 0, 0)):
    cu = bpy.data.curves.new("txt", "FONT")
    cu.body = body
    cu.font = bpy.data.fonts.load("C:/Windows/Fonts/impact.ttf")
    cu.size = size
    cu.align_x = "CENTER"
    o = bpy.data.objects.new("txt", cu)
    o.location, o.rotation_euler = loc, rot
    sc.collection.objects.link(o)
    o.data.materials.append(m)
    return o


def clear():
    for o in list(bpy.data.objects):
        bpy.data.objects.remove(o, do_unlink=True)


def render(name, w, h, samples=SAMPLES, transparent=False, denoise=True):
    sc.render.resolution_x, sc.render.resolution_y = w, h
    sc.cycles.samples = samples
    sc.cycles.use_denoising = denoise
    sc.render.film_transparent = transparent
    sc.render.filepath = os.path.join(OUT, name)
    bpy.ops.render.render(write_still=True)


# ---------------- materials ----------------

M = {
    "concrete": mat("concrete", 0x5C5848, dirt=0.6, scale=3, stretch=(3, 3, 0.5)),
    "floor": mat("floor", 0x48433A, dirt=0.65, scale=4),
    "rust": mat("rust", 0x6E3B22, rough=0.7, metal=0.5, dirt=0.55, scale=9),
    "iron": mat("iron", 0x3A3C38, rough=0.55, metal=0.7, dirt=0.5, scale=10),
    "wood": mat("wood", 0x5E4329, dirt=0.55, scale=5, stretch=(0.4, 6, 6)),
    "couch": mat("couch", 0x4C2420, dirt=0.6, scale=7),
    "tv": mat("tv", 0x3B352C, rough=0.45, dirt=0.45, scale=8),
    "paint": mat("paint", 0x7A2018, dirt=0.7, scale=5, bump=0),
    "skin": mat("skin", 0xC4B497, rough=0.6, dirt=0.25, scale=12, bump=0.1),
    "shoe": plain("shoe", 0x141210, rough=0.4),
    "glass": plain("glass", 0x0B0D0C, rough=0.15),
    "black": plain("black", 0x0E0D0B, rough=0.6),
    "cone": mat("cone", 0xD8641E, rough=0.55, dirt=0.35, scale=6),
    "white": mat("white", 0xD8D2C0, rough=0.55, dirt=0.4, scale=6),
    "tophat": plain("tophat", 0x161412, rough=0.5),
    "red": mat("red", 0xA3261E, rough=0.6, dirt=0.35, scale=6),
    "tin": mat("tin", 0x9A9A90, rough=0.35, metal=0.9, dirt=0.4, scale=8),
    "ranger": mat("ranger", 0x5A5230, dirt=0.45, scale=6),
    "kraft": mat("kraft", 0x9A7A4E, dirt=0.4, scale=5),
    "knit": mat("knit", 0xB7A24A, dirt=0.45, scale=20),
    "chair": mat("chair", 0x3D4A3E, rough=0.5, metal=0.6, dirt=0.5, scale=8),
}
screen_mat = plain("screen", 0x0B0E0D, rough=0.12, emit=0xFFFFFF, strength=0.0)
bulb_mat = plain("bulb", 0xFFE2B0, emit=0xFFC37A, strength=0.0)


# ---------------- contestants ----------------


def hat(i, p):
    z = 1.41  # top of the head
    if i == 0:  # traffic cone
        box((0.36, 0.36, 0.035), (0, -0.03, z - 0.04), M["cone"], parent=p)
        cyl(0.16, 0.42, (0, -0.03, z + 0.17), M["cone"], r2=0.02, verts=16, parent=p)
        cyl(0.118, 0.07, (0, -0.03, z + 0.13), M["white"], r2=0.096, verts=16, parent=p)
    elif i == 1:  # top hat
        cyl(0.2, 0.02, (0, -0.04, z - 0.03), M["tophat"], verts=20, parent=p)
        cyl(0.125, 0.26, (0, -0.04, z + 0.1), M["tophat"], verts=20, parent=p)
        cyl(0.128, 0.04, (0, -0.04, z + 0.0), M["red"], verts=20, parent=p)
    elif i == 2:  # welding mask
        box((0.28, 0.3, 0.32), (0, -0.05, z - 0.13), M["iron"], bevel=0.04, parent=p)
        box((0.16, 0.02, 0.06), (0, -0.205, z - 0.1), M["glass"], parent=p)
    elif i == 3:  # cap with two cans and straws
        ball(0.135, (0, -0.04, z - 0.06), M["red"], scale=(1, 1, 0.6), parent=p)
        box((0.22, 0.14, 0.015), (0, -0.17, z - 0.07), M["red"], parent=p)
        for s in (-1, 1):
            cyl(0.04, 0.11, (s * 0.16, -0.04, z - 0.02), M["tin"], parent=p)
            cyl(0.008, 0.2, (s * 0.09, -0.06, z + 0.09), M["white"], rot=(0, s * R(-50), 0), parent=p)
    elif i == 4:  # bucket
        cyl(0.155, 0.26, (0, -0.04, z - 0.02), M["tin"], r2=0.12, verts=16, parent=p)
    elif i == 5:  # ranger hat
        cyl(0.25, 0.018, (0, -0.04, z - 0.03), M["ranger"], verts=20, parent=p)
        cyl(0.13, 0.14, (0, -0.04, z + 0.04), M["ranger"], r2=0.1, verts=16, parent=p)
    elif i == 6:  # paper bag
        box((0.27, 0.27, 0.36), (0, -0.04, z - 0.12), M["kraft"], bevel=0.01, parent=p)
        for s in (-1, 1):
            box((0.05, 0.01, 0.03), (s * 0.06, -0.177, z - 0.1), M["black"], bevel=0, parent=p)
    else:  # knitted beanie
        ball(0.135, (0, -0.04, z - 0.04), M["knit"], scale=(1, 1, 0.95), parent=p)
        cyl(0.138, 0.06, (0, -0.04, z - 0.08), M["knit"], verts=20, parent=p)


def chair(p):
    c = M["chair"]
    box((0.46, 0.44, 0.035), (0, 0.02, 0.45), c, parent=p)
    box((0.44, 0.03, 0.24), (0, 0.23, 0.8), c, parent=p)
    for sx in (-1, 1):
        cyl(0.013, 0.64, (sx * 0.21, 0.23, 0.6), c, parent=p)
        for sy in (-1, 1):
            cyl(0.013, 0.45, (sx * 0.21, 0.02 + sy * 0.2, 0.225), c, parent=p)


def crew(i, loc=(0, 0, 0), rotz=0.0, with_chair=True):
    """A contestant sitting slumped, facing -Y, origin on the floor under the seat."""
    p = bpy.data.objects.new(f"crew{i}", None)
    sc.collection.objects.link(p)
    if i == 8:
        chair(p)
        p.location, p.rotation_euler = loc, (0, 0, rotz)
        return p
    h = desat(HUES[i], 0.25)
    jacket = mat(f"jacket{i}", h, dirt=0.55, scale=7)
    pants = mat(f"pants{i}", (desat(h, 0.4) & 0xFEFEFE) >> 1, dirt=0.5, scale=7)
    box((0.36, 0.3, 0.16), (0, 0, 0.53), pants, parent=p)
    for s in (-1, 1):
        box((0.15, 0.42, 0.14), (s * 0.1, -0.2, 0.53), pants, parent=p)
        box((0.12, 0.12, 0.48), (s * 0.115, -0.39, 0.27), pants, parent=p)
        box((0.13, 0.25, 0.08), (s * 0.115, -0.45, 0.04), M["shoe"], parent=p)
        box((0.11, 0.11, 0.38), (s * 0.255, 0.0, 0.9), jacket, rot=(R(10), s * R(-6), 0), parent=p)
        box((0.1, 0.36, 0.1), (s * 0.22, -0.17, 0.68), jacket, parent=p)
        ball(0.055, (s * 0.19, -0.37, 0.66), M["skin"], parent=p)
    box((0.46, 0.25, 0.58), (0, 0.05, 0.87), jacket, rot=(R(12), 0, 0), bevel=0.05, parent=p)
    box((0.2, 0.05, 0.14), (0, -0.09, 1.08), M["white"], rot=(R(12), 0, 0), parent=p)  # undershirt at the collar
    cyl(0.05, 0.12, (0, -0.02, 1.18), M["skin"], parent=p)
    ball(0.12, (0, -0.04, 1.28), M["skin"], scale=(1, 1, 1.15), parent=p)
    if i in (0, 1, 3, 5, 7):
        box((0.22, 0.03, 0.045), (0, -0.16, 1.3), M["glass"], bevel=0.008, parent=p)
    hat(i, p)
    if with_chair:
        chair(p)
    p.location, p.rotation_euler = loc, (0, 0, rotz)
    return p


# ---------------- the living room ----------------


def room():
    box((10, 0.1, 3.4), (0, 3.05, 1.7), M["concrete"], bevel=0)
    for s in (-1, 1):
        box((0.1, 9, 3.4), (s * 3.3, 0, 1.7), M["concrete"], bevel=0)
    box((10, 10, 0.1), (0, 0, -0.05), M["floor"], bevel=0)
    box((10, 10, 0.1), (0, 0, 3.15), M["black"], bevel=0)
    text("KEEP WATCHING", (-1.2, 2.995, 1.62), 0.2, M["paint"], rot=(R(90), R(-3), 0))
    # Pipes and a valve.
    cyl(0.07, 9, (0, 2.88, 1.92), M["rust"], rot=(0, R(90), 0))
    cyl(0.045, 9, (0, 2.9, 1.78), M["iron"], rot=(0, R(90), 0))
    cyl(0.06, 3.3, (-1.55, 2.88, 1.6), M["rust"])
    bpy.ops.mesh.primitive_torus_add(major_radius=0.12, minor_radius=0.018, location=(-1.55, 2.76, 1.1), rotation=(R(90), 0, 0))
    finish(bpy.context.active_object, M["red"], None, 0, smooth=True)
    # The TV: a big wood-panel console on a crate.
    box((1.6, 0.9, 0.45), (0, 2.25, 0.225), M["wood"], bevel=0.02)
    box((1.5, 0.85, 1.12), (0, 2.25, 1.01), M["tv"], bevel=0.08)
    sx, sy, sz = -0.12, 1.818, 1.04
    bpy.ops.mesh.primitive_plane_add(size=1, location=(sx, sy, sz), rotation=(R(90), 0, 0))
    scr = bpy.context.active_object
    scr.scale = (1.1, 0.8, 1)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    bev = scr.modifiers.new("round", "BEVEL")
    bev.affect, bev.width, bev.segments = "VERTICES", 0.1, 6
    scr.data.materials.append(screen_mat)
    box((1.2, 0.02, 0.9), (sx, 1.83, sz), M["black"], bevel=0.07)  # bezel, just behind the glass
    for z in (1.3, 1.12):
        cyl(0.055, 0.04, (0.58, 1.82, z), M["iron"], rot=(R(90), 0, 0), verts=16)
    box((0.18, 0.01, 0.22), (0.58, 1.825, 0.8), M["black"], bevel=0.005)
    for s in (-1, 1):
        cyl(0.007, 0.8, (s * 0.2, 2.3, 1.85), M["tin"], rot=(0, s * R(28), 0), verts=6)
    # The couch and the crew watching, seen from behind.
    cy = 0.0
    box((2.6, 0.9, 0.42), (0, cy, 0.21), M["couch"], bevel=0.05)
    box((2.4, 0.78, 0.12), (0, cy + 0.05, 0.47), M["couch"], bevel=0.05)
    box((2.6, 0.26, 0.6), (0, cy - 0.4, 0.7), M["couch"], bevel=0.08)
    for s in (-1, 1):
        box((0.24, 0.9, 0.66), (s * 1.32, cy, 0.36), M["couch"], bevel=0.07)
    for i, x in [(0, -0.92), (2, -0.52), (7, 0.52), (1, 0.92)]:
        crew(i, (x, cy + 0.05, 0.0), math.pi, with_chair=False)
    # Clutter.
    cyl(0.32, 0.92, (1.4, 2.45, 0.46), M["rust"], verts=20)
    box((0.6, 0.55, 0.55), (-1.3, 2.4, 0.275), M["wood"])
    box((0.45, 0.42, 0.4), (-1.28, 2.38, 0.75), M["wood"], rot=(0, 0, R(8)))
    for k, (x, y, z) in enumerate([(1.3, 2.4, 0.98), (1.45, 2.5, 0.98), (-1.3, 2.35, 1.01), (0.95, 1.2, 0.06), (-0.9, 1.4, 0.04)]):
        cyl(0.04, 0.12, (x, y, z), M["tin"], rot=(0, R(90) if k == 4 else 0, 0))
    # The bare bulb.
    cyl(0.005, 0.7, (0.55, 0.55, 2.8), M["black"], verts=6)
    cyl(0.022, 0.06, (0.55, 0.55, 2.43), M["black"])
    ball(0.05, (0.55, 0.55, 2.37), bulb_mat)
    bulb = light("POINT", (0.55, 0.55, 2.3), 70, (1.0, 0.72, 0.45), 0.04)
    tv = light("AREA", (sx, 1.75, sz), 160, (1, 1, 1), (1.0, 0.75), rot=(R(-90), 0, 0))
    return scr, bulb, tv, (sx, sy, sz)


def screen_emission(k):
    screen_mat.node_tree.nodes["Principled BSDF"].inputs["Emission Strength"].default_value = k


def bulb_emission(k):
    bulb_mat.node_tree.nodes["Principled BSDF"].inputs["Emission Strength"].default_value = k


scr, bulb, tv, (SX, SY, SZ) = room()
LENS, CZ, SHIFT = 55, 1.85, -0.21
cam = camera((0, -3.0, CZ), (R(90), 0, 0), LENS)
d0 = SY - cam.location.y
d1 = 1.0 * LENS / 36  # screen width just covers the view
for k in range(FRAMES):
    u = k / max(FRAMES - 1, 1)
    d = d0 * (d1 / d0) ** u
    s = (d0 - d) / (d0 - d1)
    cam.location = (SX * s, SY - d, CZ + (SZ - CZ) * s)
    cam.data.shift_y = SHIFT * (1 - s)
    # a: bulb light only
    bulb.hide_render, tv.hide_render = False, True
    bulb_emission(40)
    screen_emission(0)
    render(f"a{k:02}.png", 320, 180)
    # b: TV light only
    bulb.hide_render, tv.hide_render = True, False
    bulb_emission(0)
    screen_emission(0)  # the glass itself stays dark: its own glow would rim the bezel
    render(f"b{k:02}.png", 320, 180)
    # m: visible screen pixels (everything else is a holdout)
    tv.hide_render = True
    hold = [o for o in sc.objects if o.type in ("MESH", "FONT") and o != scr]
    for o in hold:
        o.is_holdout = True
    render(f"m{k:02}.png", 320, 180, samples=1, transparent=True, denoise=False)
    for o in hold:
        o.is_holdout = False

# ---------------- contestants, one sprite each ----------------

clear()
light("POINT", (0.2, -0.9, 2.5), 110, (1.0, 0.74, 0.48), 0.05)
light("AREA", (-1.5, -2.5, 1.2), 25, (0.55, 0.65, 0.75), (1.5, 1.5), rot=(R(70), 0, R(-35)))
light("AREA", (0.8, 1.5, 1.8), 40, (0.9, 0.8, 0.7), (0.6, 0.6), rot=(R(-60), 0, R(150)))
camera((0, -5, 1.5), (R(82), 0, 0), ortho=1.95)
for i in range(9):
    for o in [o for o in bpy.data.objects if o.name.startswith("crew") or o.parent]:
        bpy.data.objects.remove(o, do_unlink=True)
    crew(i)
    render(f"crew{i}.png", 96, 144, transparent=True)

# ---------------- the test chamber inside the TV ----------------

clear()
box((12, 0.1, 4), (0, 1.55, 2), M["concrete"], bevel=0)
box((12, 8, 0.1), (0, 0, -0.05), M["floor"], bevel=0)
for s in (-1, 1):
    box((0.1, 6, 4), (s * 4.2, -1, 2), M["concrete"], bevel=0)
text("SMILE. YOU ARE ON TV.", (0.3, 1.495, 2.15), 0.42, M["paint"], rot=(R(90), R(2), 0))
cyl(0.07, 12, (0, 1.4, 3.05), M["rust"], rot=(0, R(90), 0))
cyl(0.045, 12, (0, 1.42, 2.82), M["iron"], rot=(0, R(90), 0))
cyl(0.06, 4, (-2.7, 1.4, 2), M["rust"])
cyl(0.06, 4, (3.1, 1.4, 2), M["iron"])
for x in (-3.3, -2.0):
    box((0.5, 0.4, 0.5), (x, 1.2, 0.25), M["wood"], rot=(0, 0, R(x * 7)))
box((0.45, 0.35, 0.4), (-3.25, 1.15, 0.7), M["wood"])
cyl(0.3, 0.9, (3.4, 1.15, 0.45), M["rust"], verts=20)
cyl(0.005, 1.2, (0, 0.0, 3.4), M["black"], verts=6)
ball(0.05, (0, 0.0, 2.75), bulb_mat)
bulb_emission(40)
light("POINT", (0, 0.0, 2.68), 90, (1.0, 0.72, 0.45), 0.04)
light("AREA", (0, -3, 1.5), 8, (0.5, 0.6, 0.7), (4, 2), rot=(R(80), 0, 0))
camera((0, -3.8, 1.45), (R(84), 0, 0), 30)
render("chamber.png", 320, 180)
print("PARTYLINE art rendered to", OUT)
