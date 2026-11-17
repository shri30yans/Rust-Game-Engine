# SHINOBI  (Shirokaze Labs energy katana)  -  blender -b -P build_shinobi.py
import sys, os
# ---- BEGIN INLINED wkit.py (shared kit, also at blender/common/wkit.py) ----
# ============================================================================
# wkit.py  -  shared hard-surface weapon kit for headless Blender 4.5 LTS
# Geometry helpers, procedural source materials, PIL livery/decal painting,
# atlas bake (base colour / ORM / normal / emissive), rigid-node animation
# clips, core-glTF export, studio renders and GLB validation.
# ============================================================================
import bpy, bmesh, math, os, sys, json, struct, subprocess, time
from math import radians, sin, cos, pi
from mathutils import Vector, Matrix, Euler

_HERE = os.path.dirname(os.path.abspath(__file__)) if '__file__' in globals() else os.getcwd()
KIT_DIR = _HERE if os.path.isdir(os.path.join(_HERE, 'fonts')) else os.path.normpath(os.path.join(_HERE, '..', 'common'))
WROOT = os.environ.get('WEAPONS_ROOT', '/workspace/weapons')
FONT_DIR = os.environ.get('WEAPON_FONTS', os.path.join(KIT_DIR, 'fonts'))
FPS = 30

_DEPS = os.path.join(KIT_DIR, '_pydeps')
sys.path.insert(0, _DEPS)
try:
    from PIL import Image, ImageDraw, ImageFont, ImageFilter
except ImportError:  # Blender's bundled python has no Pillow: install it next to the kit once
    subprocess.check_call([sys.executable, '-m', 'pip', 'install', '-q', '--target', _DEPS, 'pillow'])
    import importlib; importlib.invalidate_caches(); sys.path_importer_cache.pop(_DEPS, None)
    from PIL import Image, ImageDraw, ImageFont, ImageFilter


def log(*a):
    print('[wkit]', *a, flush=True)


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    for c in (bpy.data.meshes, bpy.data.materials, bpy.data.images, bpy.data.actions):
        for x in list(c):
            c.remove(x)
    sc = bpy.context.scene
    sc.render.fps = FPS
    sc.unit_settings.system = 'METRIC'
    sc.render.engine = 'CYCLES'
    sc.cycles.device = 'CPU'


def link(ob):
    bpy.context.scene.collection.objects.link(ob)
    return ob

# ----------------------------------------------------------------------------
# 2D helpers (profiles are lists of (y, z) in metres; +Y = muzzle, +Z = up)
# ----------------------------------------------------------------------------

def arc(cy, cz, r, a0, a1, n=6):
    return [(cy + r * cos(radians(a0 + (a1 - a0) * i / n)), cz + r * sin(radians(a0 + (a1 - a0) * i / n))) for i in range(n + 1)]


def rrect(y0, z0, y1, z1, r, n=3):
    r = min(r, abs(y1 - y0) / 2 - 1e-5, abs(z1 - z0) / 2 - 1e-5)
    p = []
    p += arc(y1 - r, z0 + r, r, -90, 0, n)
    p += arc(y1 - r, z1 - r, r, 0, 90, n)
    p += arc(y0 + r, z1 - r, r, 90, 180, n)
    p += arc(y0 + r, z0 + r, r, 180, 270, n)
    return p


def circle_pts(cy, cz, r, n=16):
    return [(cy + r * cos(2 * pi * i / n), cz + r * sin(2 * pi * i / n)) for i in range(n)]

# ----------------------------------------------------------------------------
# Mesh building
# ----------------------------------------------------------------------------

def _mk(name, bm, mat):
    me = bpy.data.meshes.new(name)
    bm.normal_update()
    bm.to_mesh(me)
    bm.free()
    ob = link(bpy.data.objects.new(name, me))
    for p in me.polygons:
        p.use_smooth = True
    if mat is not None:
        me.materials.append(mat)
    return ob


def prof(name, pts, w, mat, x=0.0, bev=0.003, segs=2, cuts=(), ang=40, taper=0.0):
    """Extrude a closed (y,z) side profile along X (width w centred at x).
    taper shrinks the outer faces toward the profile centroid (sculpted side panels)."""
    bm = bmesh.new()
    cy = sum(p[0] for p in pts) / len(pts); cz = sum(p[1] for p in pts) / len(pts)
    def sh(p, t):
        return (cy + (p[0] - cy) * (1 - t), cz + (p[1] - cz) * (1 - t))
    a = [bm.verts.new((x - w / 2, *sh(p, taper))) for p in pts]
    b = [bm.verts.new((x + w / 2, *sh(p, taper))) for p in pts]
    if taper:
        ma = [bm.verts.new((x - w / 2 + w * 0.18, *p)) for p in pts]
        mb = [bm.verts.new((x + w / 2 - w * 0.18, *p)) for p in pts]
        rings = [a, ma, mb, b]
    else:
        rings = [a, b]
    bm.faces.new(a[::-1]); bm.faces.new(b)
    n = len(pts)
    for k in range(len(rings) - 1):
        r0, r1 = rings[k], rings[k + 1]
        for i in range(n):
            j = (i + 1) % n
            bm.faces.new((r0[i], r0[j], r1[j], r1[i]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    ob = _mk(name, bm, mat)
    if cuts:
        cut(ob, cuts)
    finish(ob, bev, segs, ang)
    return ob


def lathe(name, rt, mat, segs=24, axis='Y', at=(0, 0, 0), bev=0.0015, bsegs=2, ang=40, rot0=0.0, ngon_caps=True):
    """Revolve list of (radius, t) around an axis. Ends with r==0 become poles;
    if first/last r>0 and ngon_caps the ends are closed with an n-gon."""
    bm = bmesh.new()
    rings = []
    for r, t in rt:
        if r <= 1e-6:
            rings.append([bm.verts.new((0, t, 0))])
        else:
            rings.append([bm.verts.new((r * cos(2 * pi * i / segs + rot0), t, r * sin(2 * pi * i / segs + rot0))) for i in range(segs)])
    for k in range(len(rings) - 1):
        A, B = rings[k], rings[k + 1]
        if len(A) == 1 and len(B) == 1:
            continue
        if len(A) == 1:
            for i in range(segs):
                bm.faces.new((A[0], B[(i + 1) % segs], B[i]))
        elif len(B) == 1:
            for i in range(segs):
                bm.faces.new((A[i], A[(i + 1) % segs], B[0]))
        else:
            for i in range(segs):
                j = (i + 1) % segs
                bm.faces.new((A[i], A[j], B[j], B[i]))
    if ngon_caps:
        if len(rings[0]) > 1:
            bm.faces.new(rings[0])
        if len(rings[-1]) > 1:
            bm.faces.new(rings[-1][::-1])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    ob = _mk(name, bm, mat)
    M = Matrix.Identity(4)
    if axis == 'X':
        M = Matrix.Rotation(radians(-90), 4, 'Z')
    elif axis == 'Z':
        M = Matrix.Rotation(radians(90), 4, 'X')
    ob.data.transform(Matrix.Translation(at) @ M)
    finish(ob, bev, bsegs, ang)
    return ob


def tube(name, r, y0, y1, mat, x=0, z=0, segs=20, bev=0.0012, rin=None):
    """Cylinder along Y (optionally hollow with inner radius rin)."""
    if rin:
        rt = [(rin, y0), (r, y0), (r, y1), (rin, y1), (rin, y0 + 0.0001)]
        ob = lathe(name, rt, mat, segs, at=(x, 0, z), bev=bev, ngon_caps=False)
    else:
        ob = lathe(name, [(0, y0), (r, y0), (r, y1), (0, y1)], mat, segs, at=(x, 0, z), bev=bev)
    return ob


def box(name, size, center, mat, bev=0.002, segs=2, rot=None):
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    for v in bm.verts:
        v.co = Vector((v.co.x * size[0], v.co.y * size[1], v.co.z * size[2]))
    ob = _mk(name, bm, mat)
    M = Matrix.Translation(center)
    if rot:
        M = M @ Euler([radians(a) for a in rot]).to_matrix().to_4x4()
    ob.data.transform(M)
    finish(ob, bev, segs, 40)
    return ob


def xform(ob, loc=(0, 0, 0), rot=(0, 0, 0), pivot=(0, 0, 0), scale=None):
    """Bake a transform into mesh data (rotation in degrees around pivot)."""
    P = Vector(pivot)
    M = Matrix.Translation(Vector(loc) + P) @ Euler([radians(a) for a in rot]).to_matrix().to_4x4() @ Matrix.Translation(-P)
    if scale:
        M = M @ Matrix.Diagonal((*scale, 1))
    ob.data.transform(M)
    return ob


def cut(ob, cutters):
    for c in cutters:
        m = ob.modifiers.new('cut', 'BOOLEAN')
        m.operation = 'DIFFERENCE'; m.solver = 'EXACT'; m.object = c
        c.hide_render = True; c.hide_viewport = True
        apply_mods(ob)
        bpy.data.objects.remove(c, do_unlink=True)


def cutter_prof(pts, w, x=0.0):
    """Throw-away profile prism (no bevel) used as a boolean cutter."""
    bm = bmesh.new()
    a = [bm.verts.new((x - w / 2, *p)) for p in pts]
    b = [bm.verts.new((x + w / 2, *p)) for p in pts]
    bm.faces.new(a[::-1]); bm.faces.new(b)
    n = len(pts)
    for i in range(n):
        j = (i + 1) % n
        bm.faces.new((a[i], a[j], b[j], b[i]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    me = bpy.data.meshes.new('cutter'); bm.to_mesh(me); bm.free()
    return link(bpy.data.objects.new('cutter', me))


def cutter_box(size, center):
    return cutter_prof(rrect(center[1] - size[1] / 2, center[2] - size[2] / 2, center[1] + size[1] / 2, center[2] + size[2] / 2, 0.0001, 1), size[0], center[0])


def apply_mods(ob):
    dg = bpy.context.evaluated_depsgraph_get()
    me = bpy.data.meshes.new_from_object(ob.evaluated_get(dg), preserve_all_data_layers=True, depsgraph=dg)
    old = ob.data
    ob.modifiers.clear()
    ob.data = me
    bpy.data.meshes.remove(old)


def finish(ob, bev=0.002, segs=2, ang=40):
    """Crisp hard-surface finish: angle-limited bevel + sharp-by-angle + weighted normals, applied."""
    if bev and bev > 0:
        m = ob.modifiers.new('bev', 'BEVEL')
        m.width = bev; m.segments = segs; m.limit_method = 'ANGLE'; m.angle_limit = radians(ang)
        m.use_clamp_overlap = True; m.harden_normals = False; m.miter_outer = 'MITER_ARC'
        apply_mods(ob)
    ob.data.set_sharp_from_angle(angle=radians(ang))
    w = ob.modifiers.new('wn', 'WEIGHTED_NORMAL'); w.keep_sharp = True; w.weight = 50; w.mode = 'FACE_AREA_WITH_ANGLE'
    apply_mods(ob)


def mirror_x(ob, name=None):
    """Duplicate object mirrored across X=0 (for parts that sit on both sides)."""
    me = ob.data.copy()
    o2 = link(bpy.data.objects.new(name or ob.name + '_R', me))
    me.transform(Matrix.Diagonal((-1, 1, 1, 1)))
    me.flip_normals()
    return o2


def join(objs, name):
    objs = [o for o in objs if o is not None]
    if len(objs) == 1:
        objs[0].name = name; objs[0].data.name = name
        return objs[0]
    with bpy.context.temp_override(active_object=objs[0], selected_editable_objects=objs, selected_objects=objs, object=objs[0]):
        bpy.ops.object.join()
    o = objs[0]; o.name = name; o.data.name = name
    # merge duplicate material slots
    return o


def tris(ob):
    ob.data.calc_loop_triangles()
    return len(ob.data.loop_triangles)

# ----------------------------------------------------------------------------
# Node helpers
# ----------------------------------------------------------------------------
class NB:
    def __init__(self, mat):
        self.nt = mat.node_tree; self.N = self.nt.nodes; self.L = self.nt.links
    def n(self, t, **kw):
        nd = self.N.new(t)
        for k, v in kw.items():
            setattr(nd, k, v)
        return nd
    def lk(self, a, b):
        self.L.new(a, b)
    def _in(self, sock, v):
        if hasattr(v, 'is_output') or isinstance(v, bpy.types.NodeSocket):
            self.lk(v, sock)
        else:
            sock.default_value = v
    def math(self, op, a, b=0.0, clamp=False):
        m = self.n('ShaderNodeMath', operation=op, use_clamp=clamp)
        self._in(m.inputs[0], a); self._in(m.inputs[1], b)
        return m.outputs[0]
    def mr(self, v, f0, f1, t0=0.0, t1=1.0, interp='LINEAR'):
        m = self.n('ShaderNodeMapRange', interpolation_type=interp)
        self._in(m.inputs['Value'], v); m.inputs['From Min'].default_value = f0; m.inputs['From Max'].default_value = f1
        self._in(m.inputs['To Min'], t0); self._in(m.inputs['To Max'], t1)
        return m.outputs['Result']
    def mixc(self, f, a, b):
        m = self.n('ShaderNodeMix', data_type='RGBA')
        self._in(m.inputs[0], f); self._in(m.inputs[6], a); self._in(m.inputs[7], b)
        return m.outputs[2]
    def mixf(self, f, a, b):
        m = self.n('ShaderNodeMix', data_type='FLOAT')
        self._in(m.inputs[0], f); self._in(m.inputs[2], a); self._in(m.inputs[3], b)
        return m.outputs[0]
    def mulc(self, a, b):
        m = self.n('ShaderNodeMix', data_type='RGBA', blend_type='MULTIPLY')
        m.inputs[0].default_value = 1.0
        self._in(m.inputs[6], a); self._in(m.inputs[7], b)
        return m.outputs[2]

# ----------------------------------------------------------------------------
# Livery / decal painting (PIL). Each weapon draws in (y,z) metres; we render
# a LEFT view (camera at -X, muzzle on the left) and a RIGHT view.
# ----------------------------------------------------------------------------
class Sheet:
    def __init__(self, bounds, side, px_per_m=4200):
        self.y0, self.y1, self.z0, self.z1 = bounds
        self.side = side; self.s = px_per_m
        self.W = int((self.y1 - self.y0) * px_per_m); self.H = int((self.z1 - self.z0) * px_per_m)
        self.liv = Image.new('RGBA', (self.W, self.H), (0, 0, 0, 0))
        self.dec = Image.new('RGBA', (self.W, self.H), (0, 0, 0, 0))
        self.dl = ImageDraw.Draw(self.liv); self.dd = ImageDraw.Draw(self.dec)
    def P(self, y, z):
        u = (self.y1 - y) if self.side == 'L' else (y - self.y0)
        return (u * self.s, (self.z1 - z) * self.s)
    def m(self, d):
        return d * self.s
    def poly(self, pts, col, layer='liv'):
        (self.dl if layer == 'liv' else self.dd).polygon([self.P(*p) for p in pts], fill=tuple(col))
    def line(self, pts, w, col, layer='liv'):
        (self.dl if layer == 'liv' else self.dd).line([self.P(*p) for p in pts], fill=tuple(col), width=max(1, int(self.m(w))), joint='curve')
    def font(self, f, h):
        return ImageFont.truetype(os.path.join(FONT_DIR, f), max(6, int(self.m(h))))
    def text(self, s, y, z, h, col, font='ChakraPetch-Bold.ttf', anchor='mm', layer='dec', spacing=0, rot=0):
        """Text centred at (y,z), cap-height ~h metres, reads correctly from this side.
        rot (deg) tilts the text clockwise as seen from this side."""
        F = self.font(font, h * 1.38)
        if rot == 0 and spacing == 0:
            (self.dd if layer == 'dec' else self.dl).text(self.P(y, z), s, font=F, fill=tuple(col), anchor=anchor)
            return
        tmp = Image.new('RGBA', (int(self.m(h) * (len(s) + 2) * 1.4 + 40), int(self.m(h) * 2.4 + 20)), (0, 0, 0, 0))
        td = ImageDraw.Draw(tmp)
        x = 10
        for ch in s:
            td.text((x, tmp.height / 2), ch, font=F, fill=tuple(col), anchor='lm')
            x += td.textlength(ch, font=F) + self.m(spacing)
        tmp = tmp.crop((0, 0, int(x) + 10, tmp.height))
        if rot:
            tmp = tmp.rotate(-rot, expand=True, resample=Image.BICUBIC)
        cx, cz = self.P(y, z)
        (self.dec if layer == 'dec' else self.liv).alpha_composite(tmp, (int(cx - tmp.width / 2), int(cz - tmp.height / 2)))
    def tri_warn(self, y, z, h, col, mark=(0, 0, 0, 255)):
        p = [(y - h * 0.58, z - h * 0.45), (y + h * 0.58, z - h * 0.45), (y, z + h * 0.55)]
        self.poly(p, col, 'dec')
        inner = [(y - h * 0.4, z - h * 0.34), (y + h * 0.4, z - h * 0.34), (y, z + h * 0.36)]
        self.poly(inner, mark, 'dec')
        self.text('!', y, z - h * 0.06, h * 0.42, col, 'ChakraPetch-Bold.ttf')
    def chevrons(self, y, z, h, n, col, pointing=+1, gap=None, layer='dec'):
        gap = gap or h * 0.55
        for i in range(n):
            yy = y + i * gap * pointing
            t = h * 0.22
            p = [(yy, z + h / 2), (yy + pointing * h * 0.45, z), (yy, z - h / 2), (yy - pointing * t, z - h / 2), (yy + pointing * (h * 0.45 - t), z), (yy - pointing * t, z + h / 2)]
            self.poly(p, col, layer)
    def ticks(self, y0, y1, z, n, h, col, w=0.0006):
        for i in range(n + 1):
            y = y0 + (y1 - y0) * i / n
            self.line([(y, z), (y, z - (h if i % 5 == 0 else h * 0.55))], w, col, 'dec')
    def hazard(self, poly_clip, y0, y1, z0, z1, col_a, col_b, stripe=0.012, slope=1.0):
        """Diagonal hazard stripes inside a clip polygon (livery layer)."""
        W, H = self.W, self.H
        stripes = Image.new('RGBA', (W, H), (0, 0, 0, 0)); sd = ImageDraw.Draw(stripes)
        mask = Image.new('L', (W, H), 0); ImageDraw.Draw(mask).polygon([self.P(*p) for p in poly_clip], fill=255)
        sd.polygon([self.P(*p) for p in poly_clip], fill=tuple(col_a))
        k = 0; y = y0 - (z1 - z0) * slope
        while y < y1 + (z1 - z0) * slope:
            pts = [(y, z0), (y + stripe, z0), (y + stripe + (z1 - z0) * slope, z1), (y + (z1 - z0) * slope, z1)]
            if k % 2 == 0:
                sd.polygon([self.P(*p) for p in pts], fill=tuple(col_b))
            y += stripe; k += 1
        stripes.putalpha(Image.composite(stripes.getchannel('A'), Image.new('L', (W, H), 0), mask))
        self.liv.alpha_composite(stripes)
    def save(self, d, tag):
        os.makedirs(d, exist_ok=True)
        a = os.path.join(d, f'livery_{self.side}.png'); b = os.path.join(d, f'decal_{self.side}.png')
        self.liv.save(a); self.dec.save(b)
        return a, b


def paint_sheets(bounds, draw_fn, outdir):
    """draw_fn(sheet) is called for L and R sheets. Returns dict of image paths."""
    res = {}
    for side in ('L', 'R'):
        sh = Sheet(bounds, side)
        draw_fn(sh)
        res[side] = sh.save(outdir, side)
    return res

# ----------------------------------------------------------------------------
# Procedural source materials (Cycles), built for baking
# ----------------------------------------------------------------------------
SRC = {}          # material name -> dict(col, metal, rough, emit sockets)
LIV = {}          # set by setup_livery
BOUNDS = None


def setup_livery(bounds, paths, offset=(0, 0, 0)):
    """bounds are in design space; offset = design-space point that becomes the origin
    (the grip anchor), so projection matches the re-centred geometry at bake time."""
    global BOUNDS
    BOUNDS = (bounds[0] - offset[1], bounds[1] - offset[1], bounds[2] - offset[2], bounds[3] - offset[2])
    for side in ('L', 'R'):
        lp, dp = paths[side]
        LIV['liv' + side] = bpy.data.images.load(lp, check_existing=True)
        LIV['dec' + side] = bpy.data.images.load(dp, check_existing=True)


def _proj(nb, side, img_key, pos):
    y0, y1, z0, z1 = BOUNDS
    sep = nb.n('ShaderNodeSeparateXYZ'); nb.lk(pos, sep.inputs[0])
    if side == 'L':
        u = nb.mr(sep.outputs['Y'], y1, y0, 0, 1)
    else:
        u = nb.mr(sep.outputs['Y'], y0, y1, 0, 1)
    u.node.clamp = False
    v = nb.mr(sep.outputs['Z'], z0, z1, 0, 1); v.node.clamp = False
    cv = nb.n('ShaderNodeCombineXYZ'); nb.lk(u, cv.inputs[0]); nb.lk(v, cv.inputs[1])
    t = nb.n('ShaderNodeTexImage', image=LIV[img_key], extension='CLIP', interpolation='Cubic')
    nb.lk(cv.outputs[0], t.inputs[0])
    return t


def srcmat(name, base, metal=0.0, rough=0.5, livery=False, decal=True, wear=0.25, wearcol=(0.55, 0.55, 0.57),
           emit=None, pattern=None, bump=0.25, chips=0.0, ao=True, rough_var=0.08, dirt=0.0, pat=None):
    """Create a procedural PBR source material. Returns the material.
    pattern: None | 'carbon' | 'grip' | 'brushed' | 'rib' | 'jade' | 'knurl' | 'wrap' | 'tsukaito'
    pat: pattern options, e.g. tsukaito {'y0': start y (world, at bake), 'pitch': diamond length, 'xs': ellipse x scale, 'around': diamonds per turn}"""
    if name in bpy.data.materials:
        return bpy.data.materials[name]
    mat = bpy.data.materials.new(name); mat.use_nodes = True
    nb = NB(mat); nb.N.clear()
    out = nb.n('ShaderNodeOutputMaterial')
    pr = nb.n('ShaderNodeBsdfPrincipled')
    geo = nb.n('ShaderNodeNewGeometry')
    pos, nrm = geo.outputs['Position'], geo.outputs['Normal']
    sepn = nb.n('ShaderNodeSeparateXYZ'); nb.lk(nrm, sepn.inputs[0])
    nx = sepn.outputs['X']
    right = nb.math('GREATER_THAN', nx, 0.0)
    absx = nb.math('ABSOLUTE', nx)
    col = (*base, 1.0) if len(base) == 3 else base
    col_s = None
    height = None
    met_s, rough_s = metal, rough

    # ---- patterns
    if pattern == 'carbon':
        mp = nb.n('ShaderNodeMapping'); nb.lk(pos, mp.inputs[0]); mp.inputs['Scale'].default_value = (1, 1, 1)
        ck = nb.n('ShaderNodeTexChecker'); nb.lk(mp.outputs[0], ck.inputs[0]); ck.inputs['Scale'].default_value = 260
        ck.inputs['Color1'].default_value = (1, 1, 1, 1); ck.inputs['Color2'].default_value = (0, 0, 0, 1)
        wv = nb.n('ShaderNodeTexWave', wave_type='BANDS', bands_direction='DIAGONAL'); nb.lk(pos, wv.inputs[0]); wv.inputs['Scale'].default_value = 520
        f = nb.mixf(ck.outputs['Fac'], wv.outputs['Fac'], nb.math('SUBTRACT', 1.0, wv.outputs['Fac']))
        dark = tuple(c * 0.55 for c in col[:3]) + (1,)
        col_s = nb.mixc(nb.mr(f, 0, 1, 0, 1), dark, col)
        rough_s = nb.mr(f, 0, 1, rough + 0.12, rough - 0.08)
        height = nb.math('MULTIPLY', ck.outputs['Fac'], 0.6)
    elif pattern in ('grip', 'knurl'):
        vo = nb.n('ShaderNodeTexVoronoi'); nb.lk(pos, vo.inputs['Vector']); vo.inputs['Scale'].default_value = 900 if pattern == 'grip' else 1400
        height = nb.mr(vo.outputs['Distance'], 0.0, 0.6, 1.0, 0.0)
        col_s = nb.mixc(nb.math('MULTIPLY', height, 0.25), col, tuple(c * 0.6 for c in col[:3]) + (1,))
    elif pattern == 'brushed':
        mp = nb.n('ShaderNodeMapping'); nb.lk(pos, mp.inputs[0]); mp.inputs['Scale'].default_value = (900, 6, 900)
        no = nb.n('ShaderNodeTexNoise'); nb.lk(mp.outputs[0], no.inputs['Vector']); no.inputs['Scale'].default_value = 1.0; no.inputs['Detail'].default_value = 6
        rough_s = nb.mr(no.outputs['Fac'], 0.3, 0.7, rough - 0.08, rough + 0.08)
        height = nb.math('MULTIPLY', no.outputs['Fac'], 0.15)
    elif pattern == 'wrap':
        mp = nb.n('ShaderNodeMapping'); nb.lk(pos, mp.inputs[0]); mp.inputs['Scale'].default_value = (1, 1, 1)
        w1 = nb.n('ShaderNodeTexWave', wave_type='BANDS', bands_direction='DIAGONAL', wave_profile='TRI'); nb.lk(pos, w1.inputs[0]); w1.inputs['Scale'].default_value = 140
        mp2 = nb.n('ShaderNodeMapping'); nb.lk(pos, mp2.inputs[0]); mp2.inputs['Scale'].default_value = (1, -1, 1)
        w2 = nb.n('ShaderNodeTexWave', wave_type='BANDS', bands_direction='DIAGONAL', wave_profile='TRI'); nb.lk(mp2.outputs[0], w2.inputs[0]); w2.inputs['Scale'].default_value = 140
        height = nb.math('MINIMUM', w1.outputs['Fac'], w2.outputs['Fac'])
        col_s = nb.mixc(nb.mr(height, 0.0, 0.5, 0, 1), tuple(c * 0.35 for c in col[:3]) + (1,), col)
        rough_s = nb.mr(height, 0.0, 0.5, rough + 0.2, rough)
    elif pattern == 'tsukaito':
        # Even, mirror-symmetric katana handle wrap (hishimaki): two families of helical ribbons around the Y axis cross
        # into evenly spaced diamonds. a = angle around the axis (turns), t = distance along the axis (pitches).
        o = dict(y0=0.0, pitch=0.025, xs=1.0, around=2, band=0.30)
        o.update(pat or {})
        sp = nb.n('ShaderNodeSeparateXYZ'); nb.lk(pos, sp.inputs[0])
        a = nb.math('DIVIDE', nb.math('ARCTAN2', nb.math('DIVIDE', sp.outputs['X'], o['xs']), sp.outputs['Z']), 6.283185307)
        t = nb.math('DIVIDE', nb.math('SUBTRACT', sp.outputs['Y'], o['y0']), o['pitch'])
        aa = nb.math('MULTIPLY', a, float(o['around']))
        def tri(s):   # 0 at ribbon centre line, 1 halfway between ribbons
            return nb.math('MULTIPLY', nb.math('ABSOLUTE', nb.math('SUBTRACT', nb.math('FRACT', s), 0.5)), 2.0)
        d1 = tri(nb.math('ADD', t, aa)); d2 = tri(nb.math('SUBTRACT', t, aa))
        ribv = nb.math('MAXIMUM', nb.mr(d1, o['band'] + 0.06, o['band'] - 0.06, 0.0, 1.0, 'SMOOTHSTEP'),
                       nb.mr(d2, o['band'] + 0.06, o['band'] - 0.06, 0.0, 1.0, 'SMOOTHSTEP'))   # 1 on a ribbon
        twist = nb.n('ShaderNodeTexWave', wave_type='RINGS', rings_direction='Y', wave_profile='SAW')
        nb.lk(pos, twist.inputs[0]); twist.inputs['Scale'].default_value = 900; twist.inputs['Distortion'].default_value = 0
        height = nb.math('ADD', nb.math('MULTIPLY', ribv, 0.8), nb.math('MULTIPLY', nb.math('MULTIPLY', ribv, twist.outputs['Fac']), 0.12))
        hish = (0.15, 0.14, 0.12, 1.0)                     # exposed same/hishigami diamonds: warm light grey
        col_s = nb.mixc(ribv, hish, col)
        rough_s = nb.mr(ribv, 0.0, 1.0, 0.42, rough)
    elif pattern == 'rib':
        wv = nb.n('ShaderNodeTexWave', wave_type='BANDS', bands_direction='Y'); nb.lk(pos, wv.inputs[0]); wv.inputs['Scale'].default_value = 120
        wv.inputs['Distortion'].default_value = 0
        height = wv.outputs['Fac']
    elif pattern == 'jade':
        vo = nb.n('ShaderNodeTexVoronoi', feature='DISTANCE_TO_EDGE'); nb.lk(pos, vo.inputs['Vector']); vo.inputs['Scale'].default_value = 60
        no = nb.n('ShaderNodeTexNoise'); nb.lk(pos, no.inputs['Vector']); no.inputs['Scale'].default_value = 40; no.inputs['Detail'].default_value = 8
        veins = nb.mr(vo.outputs['Distance'], 0.0, 0.03, 1.0, 0.0)
        col_s = nb.mixc(nb.math('ADD', nb.math('MULTIPLY', veins, 0.6), nb.math('MULTIPLY', no.outputs['Fac'], 0.4)), col, (0.75, 1.0, 0.85, 1))

    if col_s is None:
        col_s = col

    # ---- livery colour blocks (wrap) and decals (side-facing only)
    if livery or decal:
        def side_pick(key):
            tl = _proj(nb, 'L', key + 'L', pos); tr = _proj(nb, 'R', key + 'R', pos)
            c = nb.mixc(right, tl.outputs['Color'], tr.outputs['Color'])
            a = nb.mixf(right, tl.outputs['Alpha'], tr.outputs['Alpha'])
            return c, a
        if livery:
            lc, la = side_pick('liv')
            col_s = nb.mixc(la, col_s, lc)
            # livery paint is satin, non-metal
            if not isinstance(met_s, float) or met_s > 0:
                met_s = nb.mixf(la, met_s, 0.0)
            rough_s = nb.mixf(la, rough_s, 0.42)
        if decal:
            dc, da = side_pick('dec')
            face = nb.mr(absx, 0.35, 0.7, 0, 1)
            df = nb.math('MULTIPLY', da, face)
            col_s = nb.mixc(df, col_s, dc)
            rough_s = nb.mixf(df, rough_s, 0.5)
            if not isinstance(met_s, float) or met_s > 0:
                met_s = nb.mixf(df, met_s, 0.0)

    # ---- subtle large-scale colour/roughness variation
    nz = nb.n('ShaderNodeTexNoise'); nb.lk(pos, nz.inputs['Vector']); nz.inputs['Scale'].default_value = 18; nz.inputs['Detail'].default_value = 6
    if rough_var:
        rough_s = nb.math('ADD', rough_s, nb.mr(nz.outputs['Fac'], 0.3, 0.7, -rough_var, rough_var))
    if dirt:
        col_s = nb.mixc(nb.mr(nz.outputs['Fac'], 0.45, 0.75, 0, dirt), col_s, (0.08, 0.07, 0.06, 1))

    # ---- edge wear (Bevel-normal vs true-normal) + chips
    if wear > 0 or chips > 0:
        bv = nb.n('ShaderNodeBevel'); bv.samples = 8; bv.inputs['Radius'].default_value = 0.0028
        dp = nb.n('ShaderNodeVectorMath', operation='DOT_PRODUCT'); nb.lk(bv.outputs[0], dp.inputs[0]); nb.lk(nrm, dp.inputs[1])
        edge = nb.mr(dp.outputs['Value'], 0.995, 0.93, 0, 1)
        nw = nb.n('ShaderNodeTexNoise'); nb.lk(pos, nw.inputs['Vector']); nw.inputs['Scale'].default_value = 160; nw.inputs['Detail'].default_value = 10; nw.inputs['Roughness'].default_value = 0.7
        s = nb.math('MULTIPLY', edge, nb.math('ADD', nw.outputs['Fac'], 0.25))
        t = 1.25 - wear
        wm = nb.mr(s, t, t + 0.08, 0, 1)
        if chips > 0:
            nc = nb.n('ShaderNodeTexNoise'); nb.lk(pos, nc.inputs['Vector']); nc.inputs['Scale'].default_value = 55; nc.inputs['Detail'].default_value = 12; nc.inputs['Roughness'].default_value = 0.75
            ch = nb.mr(nc.outputs['Fac'], 0.735 - chips * 0.12, 0.745 - chips * 0.12, 0, 1)
            wm = nb.math('MAXIMUM', wm, ch)
        col_s = nb.mixc(wm, col_s, (*wearcol, 1))
        met_s = nb.mixf(wm, met_s, 1.0)
        rough_s = nb.mixf(wm, rough_s, 0.32)
        hw = nb.math('MULTIPLY', wm, -0.8)
        height = hw if height is None else nb.math('ADD', height, hw)

    # ---- baked cavity AO
    if ao:
        aon = nb.n('ShaderNodeAmbientOcclusion', only_local=False, samples=12); aon.inputs['Distance'].default_value = 0.025
        col_s = nb.mulc(col_s, nb.mixc(nb.mr(aon.outputs['AO'], 0.0, 1.0, 0, 1), (0.35, 0.35, 0.35, 1), (1, 1, 1, 1)))

    rough_s = nb.math('MAXIMUM', nb.math('MINIMUM', rough_s, 1.0), 0.04) if not isinstance(rough_s, float) else rough_s

    # ---- normal detail
    fine = nb.n('ShaderNodeTexNoise'); nb.lk(pos, fine.inputs['Vector']); fine.inputs['Scale'].default_value = 900; fine.inputs['Detail'].default_value = 4
    h = nb.math('MULTIPLY', fine.outputs['Fac'], 0.08)
    height = h if height is None else nb.math('ADD', height, h)
    bmp = nb.n('ShaderNodeBump'); bmp.inputs['Strength'].default_value = bump; bmp.inputs['Distance'].default_value = 0.0004
    nb.lk(height, bmp.inputs['Height'])

    emit_s = (0, 0, 0, 1)
    if emit is not None:
        emit_s = (*emit, 1)
        if pattern == 'jade':
            emit_s = col_s

    for k, v in (('Base Color', col_s), ('Metallic', met_s), ('Roughness', rough_s), ('Emission Color', emit_s)):
        nb._in(pr.inputs[k], v)
    pr.inputs['Emission Strength'].default_value = 4.0 if emit is not None else 0.0
    nb.lk(bmp.outputs[0], pr.inputs['Normal'])
    nb.lk(pr.outputs[0], out.inputs[0])
    em = nb.n('ShaderNodeEmission')
    SRC[name] = dict(col=col_s, metal=met_s, rough=rough_s, emit=emit_s, out=out, pr=pr, em=em, nb=nb)
    return mat


def glowmat(name, color, strength=1.0):
    """Flat emissive material for FX overlay meshes (core glTF emissiveFactor only)."""
    mat = bpy.data.materials.new(name); mat.use_nodes = True
    pr = mat.node_tree.nodes['Principled BSDF']
    pr.inputs['Base Color'].default_value = (*color, 1)
    pr.inputs['Emission Color'].default_value = (*color, 1)
    pr.inputs['Emission Strength'].default_value = strength
    pr.inputs['Roughness'].default_value = 0.3
    return mat

# ----------------------------------------------------------------------------
# UV + bake
# ----------------------------------------------------------------------------

def slice_long(ob, step=0.16):
    """Bisect an object with planes perpendicular to Y every `step` metres and mark the cuts
    as UV seams, so long strips (rails, barrels) do not dominate the atlas packing."""
    bm = bmesh.new(); bm.from_mesh(ob.data)
    ys = [v.co.y for v in bm.verts]
    y0, y1 = min(ys), max(ys)
    n = int((y1 - y0) / step)
    for k in range(1, n + 1):
        y = y0 + (y1 - y0) * k / (n + 1)
        r = bmesh.ops.bisect_plane(bm, geom=bm.verts[:] + bm.edges[:] + bm.faces[:], plane_co=(0, y, 0), plane_no=(0, 1, 0))
        for e in r['geom_cut']:
            if isinstance(e, bmesh.types.BMEdge):
                e.seam = True
    bm.to_mesh(ob.data); bm.free()


def triangulate(ob):
    m = ob.modifiers.new('tri', 'TRIANGULATE'); m.keep_custom_normals = True; m.quad_method = 'BEAUTY'; m.ngon_method = 'BEAUTY'
    apply_mods(ob)


def uv_atlas(objs, margin=0.004, step=0.16):
    for o in objs:
        slice_long(o, step)
        w = o.modifiers.new('wn', 'WEIGHTED_NORMAL'); w.keep_sharp = True; w.weight = 50; w.mode = 'FACE_AREA_WITH_ANGLE'
        apply_mods(o)
        triangulate(o)
    for o in bpy.context.scene.objects:
        o.select_set(False)
    for o in objs:
        o.select_set(True)
        if not o.data.uv_layers:
            o.data.uv_layers.new(name='UVMap')
    bpy.context.view_layer.objects.active = objs[0]
    bpy.ops.object.mode_set(mode='EDIT')
    bpy.ops.mesh.select_all(action='SELECT')
    bpy.ops.uv.smart_project(angle_limit=radians(50), island_margin=margin, area_weight=0.0, correct_aspect=True, scale_to_bounds=False)
    bpy.ops.uv.select_all(action='SELECT')
    bpy.ops.uv.seams_from_islands(mark_seams=True)
    bpy.ops.uv.unwrap(method='CONFORMAL', margin=margin)
    bpy.ops.uv.select_all(action='SELECT')
    bpy.ops.uv.average_islands_scale()
    bpy.ops.uv.pack_islands(margin=margin, rotate=True, shape_method='CONCAVE')
    bpy.ops.object.mode_set(mode='OBJECT')


def _img(name, res, data):
    im = bpy.data.images.new(name, res, res, alpha=False, float_buffer=False, is_data=data)
    if data:
        im.colorspace_settings.name = 'Non-Color'
    return im


def bake_atlas(objs, res, texdir, tag, samples=16):
    """Bake every source material on objs into one atlas: BaseColor, ORM, Normal, Emissive."""
    import numpy as np
    sc = bpy.context.scene
    sc.render.engine = 'CYCLES'; sc.cycles.samples = samples; sc.cycles.use_denoising = False
    sc.render.bake.margin = 12; sc.render.bake.use_clear = True; sc.render.bake.target = 'IMAGE_TEXTURES'
    for o in sc.objects:
        o.select_set(o in objs)
    bpy.context.view_layer.objects.active = objs[0]
    mats = {s.material for o in objs for s in o.material_slots if s.material}
    imgs = {k: _img(f'{tag}_{k}', res, k in ('metal', 'rough', 'normal')) for k in ('col', 'metal', 'rough', 'emit', 'normal')}
    tex_nodes = {}
    for m in mats:
        nt = m.node_tree
        tn = nt.nodes.new('ShaderNodeTexImage'); nt.nodes.active = tn; tex_nodes[m.name] = tn
    def run(kind, chan):
        for m in mats:
            S = SRC[m.name]; nb = S['nb']; tn = tex_nodes[m.name]; tn.image = imgs[chan]; nb.nt.nodes.active = tn
            if kind == 'EMIT':
                for l in list(S['out'].inputs[0].links):
                    nb.L.remove(l)
                v = S[chan]
                if isinstance(v, (int, float)):
                    v = (v, v, v, 1.0)
                for l in list(S['em'].inputs['Color'].links):
                    nb.L.remove(l)
                nb._in(S['em'].inputs['Color'], v)
                S['em'].inputs['Strength'].default_value = 1.0
                nb.lk(S['em'].outputs[0], S['out'].inputs[0])
            else:
                for l in list(S['out'].inputs[0].links):
                    nb.L.remove(l)
                nb.lk(S['pr'].outputs[0], S['out'].inputs[0])
        t = time.time()
        if kind == 'EMIT':
            bpy.ops.object.bake(type='EMIT')
        else:
            bpy.ops.object.bake(type='NORMAL', normal_space='TANGENT')
        log(f'bake {chan} {res}px {time.time() - t:.1f}s')
    for ch in ('col', 'metal', 'rough', 'emit'):
        run('EMIT', ch)
    run('NORMAL', 'normal')
    os.makedirs(texdir, exist_ok=True)
    # ORM pack: R = 1 (AO already in colour), G = roughness, B = metallic
    n = res * res * 4
    r = np.empty(n, np.float32); imgs['rough'].pixels.foreach_get(r)
    mt = np.empty(n, np.float32); imgs['metal'].pixels.foreach_get(mt)
    orm = np.ones(n, np.float32); orm[1::4] = r[0::4]; orm[2::4] = mt[0::4]
    im_orm = _img(f'{tag}_ORM', res, True); im_orm.pixels.foreach_set(orm)
    out = {}
    for key, im, fn in (('col', imgs['col'], 'BaseColor'), ('orm', im_orm, 'ORM'), ('normal', imgs['normal'], 'Normal'), ('emit', imgs['emit'], 'Emissive')):
        p = os.path.join(texdir, f'{tag}_{fn}_{res}.png')
        im.filepath_raw = p; im.file_format = 'PNG'; im.save()
        out[key] = p
    for m in mats:  # restore preview shading
        S = SRC[m.name]
        for l in list(S['out'].inputs[0].links):
            S['nb'].L.remove(l)
        S['nb'].lk(S['pr'].outputs[0], S['out'].inputs[0])
    return out


def downscale(paths, res):
    out = {}
    for k, p in paths.items():
        im = Image.open(p)
        im = im.resize((res, res), Image.LANCZOS)
        q = p.replace('_2048.png', f'_{res}.png')
        im.save(q, optimize=True)
        out[k] = q
    return out


def final_material(name, tex):
    """Core glTF metal/rough material driven by the baked atlas. Emissive strength 1.0
    so the exporter writes plain emissiveFactor + emissiveTexture (no extension)."""
    mat = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    mat.use_nodes = True; nt = mat.node_tree; nt.nodes.clear()
    out = nt.nodes.new('ShaderNodeOutputMaterial'); pr = nt.nodes.new('ShaderNodeBsdfPrincipled')
    nt.links.new(pr.outputs[0], out.inputs[0])
    def tn(p, data):
        t = nt.nodes.new('ShaderNodeTexImage'); t.image = bpy.data.images.load(p, check_existing=True)
        if data:
            t.image.colorspace_settings.name = 'Non-Color'
        return t
    c = tn(tex['col'], False); nt.links.new(c.outputs[0], pr.inputs['Base Color'])
    o = tn(tex['orm'], True); sp = nt.nodes.new('ShaderNodeSeparateColor'); nt.links.new(o.outputs[0], sp.inputs[0])
    nt.links.new(sp.outputs[1], pr.inputs['Roughness']); nt.links.new(sp.outputs[2], pr.inputs['Metallic'])
    nm = tn(tex['normal'], True); nmap = nt.nodes.new('ShaderNodeNormalMap'); nt.links.new(nm.outputs[0], nmap.inputs['Color']); nt.links.new(nmap.outputs[0], pr.inputs['Normal'])
    e = tn(tex['emit'], False); nt.links.new(e.outputs[0], pr.inputs['Emission Color'])
    pr.inputs['Emission Strength'].default_value = 1.0
    mat['tex'] = json.dumps(tex)
    return mat


def swap_textures(mat, tex):
    for t in mat.node_tree.nodes:
        if t.type == 'TEX_IMAGE':
            for k, p in tex.items():
                cur = t.image.filepath
                if os.path.basename(cur).split('_')[-2] == os.path.basename(p).split('_')[-2]:
                    cs = t.image.colorspace_settings.name
                    t.image = bpy.data.images.load(p, check_existing=True); t.image.colorspace_settings.name = cs

# ----------------------------------------------------------------------------
# Hierarchy + animation clips (rigid TRS node animation, merged by NLA track name)
# ----------------------------------------------------------------------------
ANIM = []        # animated objects
REST = {}
CLIPS = []       # (name, length_frames, loop, description)


def empty(name, loc=(0, 0, 0), parent=None, size=0.03):
    e = link(bpy.data.objects.new(name, None)); e.empty_display_size = size; e.location = loc
    if parent:
        e.parent = parent
    return e


def parent_keep(child, parent):
    """Parent while keeping world position; mesh data is re-expressed so the child's
    local origin becomes `pivot` (set child.location first)."""
    mw = child.matrix_world.copy()
    child.parent = parent
    child.matrix_parent_inverse = Matrix.Identity(4)
    child.matrix_world = mw


def set_origin(ob, pivot):
    """Move object origin to world point pivot without moving geometry."""
    P = Vector(pivot)
    ob.data.transform(Matrix.Translation(-P))
    ob.location = ob.location + P


def animated(*objs):
    for o in objs:
        o.rotation_mode = 'XYZ'
        ANIM.append(o)
        REST[o.name] = (o.location.copy(), o.rotation_euler.copy(), o.scale.copy())


def clip(name, length, keys, loop=False, desc=''):
    """keys: {obj: [(frame, {'l':(dx,dy,dz), 'r':(deg x,y,z), 's':(sx,sy,sz)}), ...]} offsets from rest.
    Every animated node gets keys (rest at 0/length if unspecified) so clips are self-contained."""
    CLIPS.append((name, length, loop, desc))
    km = {(k if isinstance(k, str) else k.name): v for k, v in keys.items()}
    for ob in ANIM:
        l0, r0, s0 = REST[ob.name]
        ks = sorted(km.get(ob.name, []), key=lambda x: x[0])
        frames = [f for f, _ in ks]
        if 0 not in frames:
            ks.insert(0, (0, {}))
        if length not in frames:
            ks.append((length, ks[0][1] if loop else {}))
        ad = ob.animation_data_create()
        act = bpy.data.actions.new(f'{name}__{ob.name}')
        ad.action = act
        for f, d in ks:
            ob.location = l0 + Vector(d.get('l', (0, 0, 0)))
            r = d.get('r', (0, 0, 0)); ob.rotation_euler = Euler((r0.x + radians(r[0]), r0.y + radians(r[1]), r0.z + radians(r[2])))
            s = d.get('s', (1, 1, 1)); s = (s, s, s) if isinstance(s, (int, float)) else s
            ob.scale = Vector((s0.x * s[0], s0.y * s[1], s0.z * s[2]))
            for p in ('location', 'rotation_euler', 'scale'):
                ob.keyframe_insert(p, frame=f)
            if d.get('const'):
                pass
        # interpolation tweaks: keys tagged 'lin' get linear interpolation (spins)
        lin_frames = {f for f, d in ks if d.get('lin')}
        if lin_frames:
            for fc in _fcurves(act):
                for kp in fc.keyframe_points:
                    if int(round(kp.co.x)) in lin_frames:
                        kp.interpolation = 'LINEAR'
        ad.action = None
        tr = ad.nla_tracks.new(); tr.name = name
        st = tr.strips.new(name, 0, act); st.extrapolation = 'NOTHING'
        ob.location, ob.rotation_euler, ob.scale = l0.copy(), r0.copy(), s0.copy()


def _fcurves(act):
    try:
        return list(act.fcurves)
    except Exception:
        out = []
        for layer in act.layers:
            for strip in layer.strips:
                for cb in strip.channelbags:
                    out += list(cb.fcurves)
        return out


def spin_keys(n_frames, deg_total, axis=1, start=0, step=5):
    """Linear spin keys (offset rotation) for a node, sampled every `step` frames."""
    ks = []
    f = start
    while f <= start + n_frames:
        a = deg_total * (f - start) / n_frames
        r = [0, 0, 0]; r[axis] = a
        ks.append((f, {'r': tuple(r), 'lin': True}))
        f += step
    return ks

# ----------------------------------------------------------------------------
# Export + validation
# ----------------------------------------------------------------------------

def export_glb(path, objs):
    for o in bpy.context.scene.objects:
        o.select_set(o in objs)
    bpy.ops.export_scene.gltf(
        filepath=path, export_format='GLB', use_selection=True, export_yup=True, export_apply=True,
        export_texcoords=True, export_normals=True, export_tangents=False, export_materials='EXPORT',
        export_image_format='AUTO', export_cameras=False, export_lights=False, export_extras=False,
        export_animations=True, export_animation_mode='NLA_TRACKS', export_force_sampling=True,
        export_frame_step=1, export_optimize_animation_size=False, export_anim_slide_to_zero=True,
        export_pointer_animation=False, export_morph=False, export_skins=False,
        export_draco_mesh_compression_enable=False)
    log('exported', path, os.path.getsize(path) // 1024, 'KB')


def inspect_glb(path):
    """Pure-python GLB inspection: tris, materials, images, animations, extensions."""
    with open(path, 'rb') as f:
        data = f.read()
    jl = struct.unpack('<I', data[12:16])[0]
    j = json.loads(data[20:20 + jl])
    binoff = 20 + jl + 8
    acc = j.get('accessors', [])
    t = 0; prims = 0
    for m in j.get('meshes', []):
        for p in m['primitives']:
            prims += 1
            mode = p.get('mode', 4)
            assert mode == 4, 'non-triangle primitive'
            t += acc[p['indices']]['count'] // 3 if 'indices' in p else acc[p['attributes']['POSITION']]['count'] // 3
    imgs = []
    for im in j.get('images', []):
        bv = j['bufferViews'][im['bufferView']]
        b = data[binoff + bv.get('byteOffset', 0): binoff + bv.get('byteOffset', 0) + 24]
        w, h = struct.unpack('>II', b[16:24])
        imgs.append((im.get('name'), im.get('mimeType'), w, h))
    anims = []
    for a in j.get('animations', []):
        dur = max(acc[s['input']]['max'][0] for s in a['samplers'])
        paths = sorted({c['target']['path'] for c in a['channels']})
        nodes = sorted({j['nodes'][c['target']['node']]['name'] for c in a['channels'] if 'node' in c['target']})
        interp = sorted({s.get('interpolation', 'LINEAR') for s in a['samplers']})
        anims.append(dict(name=a['name'], seconds=round(dur, 4), frames=int(round(dur * FPS)), channels=len(a['channels']), paths=paths, nodes=nodes, interp=interp))
    return dict(file=os.path.basename(path), kb=len(data) // 1024, tris=t, primitives=prims, meshes=len(j.get('meshes', [])),
                materials=[m['name'] for m in j.get('materials', [])], images=imgs, animations=anims,
                extensionsUsed=j.get('extensionsUsed', []), extensionsRequired=j.get('extensionsRequired', []),
                scenes=len(j.get('scenes', [])), nodes=[n.get('name') for n in j['nodes']], skins=len(j.get('skins', [])))


def node_tree_lines(path):
    with open(path, 'rb') as f:
        data = f.read()
    jl = struct.unpack('<I', data[12:16])[0]
    j = json.loads(data[20:20 + jl])
    lines = []
    def walk(i, d):
        n = j['nodes'][i]
        kind = 'mesh' if 'mesh' in n else 'node'
        lines.append('  ' * d + f"- {n.get('name')} ({kind})")
        for c in n.get('children', []):
            walk(c, d + 1)
    for r in j['scenes'][j.get('scene', 0)]['nodes']:
        walk(r, 0)
    return lines

# ----------------------------------------------------------------------------
# Studio rendering (Cycles CPU) - premium dark product shot
# ----------------------------------------------------------------------------

def studio(target_objs, accent=(0.2, 0.8, 1.0), cam_dir=(-1.0, 0.32, 0.30), lens=60, res=(1600, 900), emit_boost=2.2, glow_boost=3.0):
    sc = bpy.context.scene
    bpy.context.view_layer.update()
    sc.render.engine = 'CYCLES'; sc.cycles.device = 'CPU'
    sc.render.resolution_x, sc.render.resolution_y = res; sc.render.resolution_percentage = 100
    sc.view_settings.view_transform = 'AgX'
    try:
        sc.view_settings.look = os.environ.get('AGX_LOOK', 'AgX - Punchy')
    except Exception:
        pass
    sc.view_settings.exposure = -0.25
    w = bpy.data.worlds.new('W'); sc.world = w; w.use_nodes = True
    w.node_tree.nodes['Background'].inputs[0].default_value = (0.004, 0.0045, 0.006, 1); w.node_tree.nodes['Background'].inputs[1].default_value = 1.0
    # bounds
    pts = [o.matrix_world @ Vector(c) for o in target_objs if o.type == 'MESH' for c in o.bound_box]
    mn = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
    mx = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
    ctr = (mn + mx) / 2; L = (mx - mn).length
    # backdrop sweep
    fz = mn.z - 0.55
    prof_ = [(1.0 + 0.6 * sin(radians(a)), fz + 0.6 - 0.6 * cos(radians(a))) for a in range(0, 91, 10)] + [(1.6, 3.0)]   # back wall only (no lit floor)
    me = bpy.data.meshes.new('Backdrop')
    verts = []; faces = []
    for k, (u, zz) in enumerate(prof_):
        verts += [(-4, u, zz), (4, u, zz)]
    for k in range(len(prof_) - 1):
        a = 2 * k; faces.append((a, a + 1, a + 3, a + 2))
    me.from_pydata(verts, [], faces)
    for p in me.polygons:
        p.use_smooth = True
    bd = link(bpy.data.objects.new('Backdrop', me))
    # rotate so the sweep curves up behind the gun relative to camera (camera at -X): wall at +X
    bd.rotation_euler = (0, 0, radians(-90)); bd.location = (ctr.x, ctr.y, 0)
    bm_ = bpy.data.materials.new('BackdropMat'); bm_.use_nodes = True
    p = bm_.node_tree.nodes['Principled BSDF']; p.inputs['Base Color'].default_value = (0.0025, 0.0027, 0.0032, 1); p.inputs['Roughness'].default_value = 0.6; p.inputs['Specular IOR Level'].default_value = 0.08
    bd.data.materials.append(bm_)
    def area(name, loc, size, power, color=(1, 1, 1), shape='RECTANGLE', sy=None):
        ld = bpy.data.lights.new(name, 'AREA'); ld.energy = power; ld.color = color; ld.shape = shape; ld.size = size
        if sy:
            ld.size_y = sy
        ld.spread = radians(55)
        lo = link(bpy.data.objects.new(name, ld)); lo.location = ctr + Vector(loc)
        d = (ctr - lo.location); lo.rotation_euler = d.to_track_quat('-Z', 'Y').to_euler()
        return lo
    s = L
    area('Key', (-0.6 * s, 0.25 * s, 0.9 * s), 1.1 * s, 4.5 * s * s, (1.0, 0.97, 0.93))
    area('Fill', (-1.2 * s, -0.6 * s, 0.1 * s), 1.4 * s, 1.0 * s * s, (0.85, 0.9, 1.0))
    area('RimTop', (0.6 * s, -0.1 * s, 0.75 * s), 0.12 * s, 30 * s * s, (0.9, 0.95, 1.0), 'RECTANGLE', 1.4 * s)
    area('RimBack', (0.75 * s, 0.6 * s, 0.15 * s), 0.12 * s, 14 * s * s, tuple(0.6 + 0.4 * c for c in accent), 'RECTANGLE', 1.0 * s)
    area('Kick', (-0.4 * s, -0.9 * s, 0.5 * s), 0.3 * s, 3 * s * s, (1, 1, 1))
    # camera
    cd = bpy.data.cameras.new('Cam'); cd.lens = lens; cd.dof.use_dof = False
    cam = link(bpy.data.objects.new('Cam', cd)); sc.camera = cam
    dirv = Vector(cam_dir).normalized()
    cam.rotation_euler = (-dirv).to_track_quat('-Z', 'Y').to_euler()
    corners = [Vector((x, y, z)) for x in (mn.x, mx.x) for y in (mn.y, mx.y) for z in (mn.z, mx.z)]
    cam.location = ctr + dirv * 3
    bpy.context.view_layer.update()
    co = [c for v in corners for c in v]
    loc, scale = cam.camera_fit_coords(bpy.context.evaluated_depsgraph_get(), co)
    cam.location = ctr + (Vector(loc) - ctr) * 1.12
    # boost emissives for the beauty render only (exported files keep strength 1.0)
    for m in bpy.data.materials:
        if m.use_nodes:
            for n in m.node_tree.nodes:
                if n.type == 'BSDF_PRINCIPLED' and n.inputs['Emission Strength'].default_value > 0:
                    n.inputs['Emission Strength'].default_value = glow_boost if m.name.endswith('_Glow') else emit_boost
    # compositor bloom
    sc.use_nodes = True; ct = sc.node_tree; ct.nodes.clear()
    rl = ct.nodes.new('CompositorNodeRLayers'); g = ct.nodes.new('CompositorNodeGlare'); comp = ct.nodes.new('CompositorNodeComposite')
    try:
        g.glare_type = 'FOG_GLOW'; g.quality = 'MEDIUM'
    except Exception:
        pass
    for k, v in (('threshold', 0.9), ('size', 7), ('mix', -0.55)):
        try:
            setattr(g, k, v)
        except Exception:
            pass
    for k, v in (('Threshold', 0.9), ('Strength', 0.55), ('Size', 0.6)):
        if k in g.inputs:
            try:
                g.inputs[k].default_value = v
            except Exception:
                pass
    ct.links.new(rl.outputs['Image'], g.inputs['Image']); ct.links.new(g.outputs['Image'], comp.inputs['Image'])
    return cam, ctr, L


def render_still(path, samples=128):
    sc = bpy.context.scene
    sc.cycles.samples = samples; sc.cycles.use_denoising = True; sc.cycles.denoiser = 'OPENIMAGEDENOISE'
    sc.cycles.use_adaptive_sampling = True; sc.cycles.adaptive_threshold = 0.02
    sc.render.image_settings.file_format = 'PNG'; sc.render.filepath = path
    t = time.time(); bpy.ops.render.render(write_still=True); log('render', path, f'{time.time() - t:.1f}s')


def build_preview_timeline(root, seq, turn_frames=120):
    """Mute export tracks and lay clips end-to-end on a PREVIEW track per animated node.
    Root does a 360 turntable for the first turn_frames while Idle loops."""
    f = turn_frames
    for ob in ANIM:
        for tr in ob.animation_data.nla_tracks:
            tr.mute = True
        tr = ob.animation_data.nla_tracks.new(); tr.name = 'PREVIEW'
        cur = 0
        idle = bpy.data.actions.get(f'Idle__{ob.name}')
        il = next(c[1] for c in CLIPS if c[0] == 'Idle')
        while cur < turn_frames:
            st = tr.strips.new(f'Idle{cur}', cur, idle); cur += il
            if cur > turn_frames:
                st.action_frame_end = il - (cur - turn_frames); cur = turn_frames
        for name in seq:
            a = bpy.data.actions.get(f'{name}__{ob.name}')
            ln = next(c[1] for c in CLIPS if c[0] == name)
            tr.strips.new(f'{name}{cur}', cur, a); cur += ln
    total = turn_frames + sum(next(c[1] for c in CLIPS if c[0] == n) for n in seq)
    root.rotation_mode = 'XYZ'
    rz = root.rotation_euler.z
    root.keyframe_insert('rotation_euler', frame=0)
    root.rotation_euler.z = rz + radians(360); root.keyframe_insert('rotation_euler', frame=turn_frames)
    for fc in _fcurves(root.animation_data.action):
        for kp in fc.keyframe_points:
            kp.interpolation = 'LINEAR'
    sc = bpy.context.scene; sc.frame_start = 0; sc.frame_end = total - 1
    # labels for overlay
    labels = [(0, turn_frames, 'Turntable + Idle')]
    c = turn_frames
    for n in seq:
        ln = next(x[1] for x in CLIPS if x[0] == n); labels.append((c, c + ln, n)); c += ln
    return total, labels


def render_preview(mp4, framesdir, labels, res=(800, 450), samples=8):
    sc = bpy.context.scene
    sc.render.resolution_x, sc.render.resolution_y = res
    sc.cycles.samples = samples; sc.cycles.use_denoising = True; sc.cycles.use_adaptive_sampling = True; sc.cycles.adaptive_threshold = 0.05
    sc.cycles.max_bounces = 4; sc.cycles.glossy_bounces = 2; sc.cycles.diffuse_bounces = 1; sc.cycles.transmission_bounces = 1
    sc.render.use_persistent_data = True
    os.makedirs(framesdir, exist_ok=True)
    sc.render.image_settings.file_format = 'PNG'; sc.render.filepath = os.path.join(framesdir, 'f_')
    t = time.time(); bpy.ops.render.render(animation=True); log('preview frames', f'{time.time() - t:.1f}s')
    font = os.path.join(FONT_DIR, 'ChakraPetch-SemiBold.ttf')
    dt = []
    for a, b, name in labels:
        dt.append(f"drawtext=fontfile={font}:text='{name}':x=28:y=h-52:fontsize=26:fontcolor=white@0.85:enable='between(n,{a},{b - 1})'")
    vf = ','.join(dt)
    cmd = ['ffmpeg', '-y', '-loglevel', 'error', '-framerate', str(FPS), '-start_number', '0', '-i', os.path.join(framesdir, 'f_%04d.png'),
           '-vf', vf, '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-crf', '20', '-movflags', '+faststart', mp4]
    subprocess.check_call(cmd)
    log('preview', mp4)


def srgb(*c):
    """sRGB 0-1 triple -> linear (Blender node colours are linear)."""
    def f(x):
        return x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4
    return tuple(f(x) for x in c)


def sect(name, pts, y0, y1, mat, bev=0.002, segs=2, ang=40, cuts=()):
    """Extrude a closed (x,z) cross-section along Y from y0 to y1."""
    bm = bmesh.new()
    a = [bm.verts.new((p[0], y0, p[1])) for p in pts]
    b = [bm.verts.new((p[0], y1, p[1])) for p in pts]
    bm.faces.new(a[::-1]); bm.faces.new(b)
    n = len(pts)
    for i in range(n):
        j = (i + 1) % n
        bm.faces.new((a[i], a[j], b[j], b[i]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    ob = _mk(name, bm, mat)
    if cuts:
        cut(ob, cuts)
    finish(ob, bev, segs, ang)
    return ob


def chamfer_rect(x0, z0, x1, z1, c):
    return [(x0 + c, z0), (x1 - c, z0), (x1, z0 + c), (x1, z1 - c), (x1 - c, z1), (x0 + c, z1), (x0, z1 - c), (x0, z0 + c)]


def quick_render(path, objs, res=(1280, 720), samples=24, **kw):
    cam, ctr, L = studio(objs, res=res, **kw)
    render_still(path, samples)


def run_pipeline(W, TAG, root, meshes, fx, preview_seq, accent=(0.2, 0.8, 1.0), cam_dir=(-1.0, 0.32, 0.30), lens=60):
    quick = os.environ.get('QUICK') == '1'
    outdir = os.path.join(WROOT, 'blender', W); rdir = os.path.join(WROOT, 'renders'); edir = os.path.join(WROOT, 'export')
    for d in (outdir, rdir, edir):
        os.makedirs(d, exist_ok=True)
    allobjs = [root] + list(root.children_recursive)
    log('tris total', sum(tris(o) for o in meshes + fx), {o.name: tris(o) for o in meshes + fx})
    if quick:
        studio(meshes + fx, accent=accent, cam_dir=cam_dir, lens=lens, res=(1280, 720))
        render_still(os.path.join(outdir, f'{W}_quick.png'), 32)
        return
    texdir = os.path.join(edir, 'textures', W)
    uv_atlas(meshes)
    tex2 = bake_atlas(meshes, 2048, texdir, TAG, samples=int(os.environ.get('BAKE_SAMPLES', '16')))
    fm = final_material(f'M_{TAG}', tex2)
    for o in meshes:
        o.data.materials.clear(); o.data.materials.append(fm)
    # remove unused source materials so they never reach the GLB
    glb = os.path.join(edir, f'{W}.glb'); glbm = os.path.join(edir, f'{W}_mobile.glb')
    export_glb(glb, allobjs)
    tex1 = downscale(tex2, 1024)
    swap_textures(fm, tex1); export_glb(glbm, allobjs); swap_textures(fm, tex2)
    rep = {'desktop': inspect_glb(glb), 'mobile': inspect_glb(glbm), 'clips': [dict(name=c[0], frames=c[1], loop=c[2], desc=c[3]) for c in CLIPS],
           'hierarchy': node_tree_lines(glb), 'textures_2k': tex2, 'textures_1k': tex1}
    with open(os.path.join(edir, f'{W}_report.json'), 'w') as f:
        json.dump(rep, f, indent=1)
    log('REPORT', json.dumps({k: rep['desktop'][k] for k in ('tris', 'materials', 'extensionsUsed', 'skins')}), [a['name'] + ':' + str(a['frames']) for a in rep['desktop']['animations']])
    bpy.context.scene['wk_clips'] = json.dumps(CLIPS); bpy.context.scene['wk_anim'] = json.dumps([o.name for o in ANIM])
    bpy.context.scene['wk_opts'] = json.dumps(dict(root=root.name, meshes=[o.name for o in meshes], fx=[o.name for o in fx], accent=list(accent), cam_dir=list(cam_dir), lens=lens, seq=preview_seq))
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(outdir, f'{W}.blend'))
    studio(meshes + fx, accent=accent, cam_dir=cam_dir, lens=lens)
    render_still(os.path.join(rdir, f'{W}_hero.png'), int(os.environ.get('HERO_SAMPLES', '128')))
    if os.environ.get('SKIP_PREVIEW') == '1':
        return
    total, labels = build_preview_timeline(root, preview_seq)
    render_preview(os.path.join(rdir, f'{W}_preview.mp4'), os.path.join(outdir, 'preview_frames'), labels,
                   samples=int(os.environ.get('PREVIEW_SAMPLES', '8')))

# ---- END INLINED wkit.py ----

W, TAG = 'shinobi', 'SHINOBI'
reset()
OUT = os.path.join(WROOT, 'blender', W)
G = Vector((0, -0.13, 0.0))        # hand position on the tsuka
LGREY = (200, 204, 206, 255); INK = (22, 24, 24, 255); GOLDC = (200, 160, 80, 255); JADE = (60, 220, 150, 255)
BOUNDS_ = (-0.34, 0.78, -0.10, 0.12)
L = 0.735

def ztop(y):
    return 0.017 + 0.030 * (max(y, 0) / L) ** 2

def draw(sh):
    sh.text('Shirokaze Labs', 0.150, ztop(0.15) - 0.008, 0.0062, LGREY, 'Michroma-Regular.ttf')
    sh.text('03', 0.050, ztop(0.05) - 0.009, 0.0085, LGREY, 'Michroma-Regular.ttf')
    sh.line([(0.09, ztop(0.09) - 0.0035), (0.62, ztop(0.62) - 0.0035)], 0.0006, (120, 124, 126, 255), 'dec')   # bo-hi line
    sh.line([(0.24, ztop(0.24) - 0.016), (0.60, ztop(0.60) - 0.016)], 0.0005, (90, 96, 96, 255), 'dec')

paths = paint_sheets(BOUNDS_, draw, os.path.join(OUT, 'livery'))
setup_livery(BOUNDS_, paths, G)

BLADE = srcmat('BladeBlack', srgb(0.06, 0.065, 0.068), metal=0.9, rough=0.22, wear=0.12, wearcol=srgb(0.7, 0.72, 0.74), pattern='brushed')
JADEM = srcmat('Jade', srgb(0.10, 0.62, 0.36), rough=0.15, emit=srgb(0.04, 0.70, 0.32), decal=False, wear=0, ao=False, pattern='jade')
GOLD = srcmat('Gold', srgb(0.82, 0.62, 0.30), metal=1.0, rough=0.25, wear=0.1, decal=False)
DARK = srcmat('DarkMetal', srgb(0.07, 0.072, 0.075), metal=1.0, rough=0.3, wear=0.2, decal=False)
# Tsuka wrap: even, mirror-symmetric diamond wrap. 10 diamonds over the exposed handle (y -0.300..-0.034, shifted by -G at bake time).
TS0, TS1, TSR, TSX = -0.300, -0.034, 0.0160, 0.72
WRAP = srcmat('Wrap', srgb(0.075, 0.078, 0.082), rough=0.6, pattern='tsukaito', bump=0.6, wear=0.0, decal=False, ao=False,
              pat=dict(y0=TS0 - G.y, pitch=(TS1 - TS0) / 10, xs=TSX, around=2, band=0.36))
GLOW = glowmat(f'M_{TAG}_Glow', srgb(0.15, 0.95, 0.50))

P = []
N = 14
ys = [L * 0.94 * i / N for i in range(N + 1)]
# blade body (dark) - stops at the chisel line; jade edge runs full length and forms the tanto tip
body = [(y, ztop(y)) for y in ys] + [(0.655, ztop(0.655) - 0.021)] + [(y, ztop(y) - 0.0235) for y in reversed(ys[:-1])] + [(0.0, ztop(0) - 0.0235)]
body = [(y, z) for (y, z) in body if not (y > 0.66 and z < ztop(y) - 0.005)]
bodyp = [(y, ztop(y)) for y in ys if y <= 0.69] + [(0.692, ztop(0.692) - 0.003), (0.650, ztop(0.65) - 0.023)] + [(y, ztop(y) - 0.0235) for y in reversed(ys) if y < 0.64] 
P.append(prof('blade', bodyp, 0.0072, BLADE, bev=0.0012, segs=1, taper=0.22))
edge = [(y, ztop(y) - 0.022) for y in ys if y < 0.64] + [(0.650, ztop(0.65) - 0.022), (0.692, ztop(0.692) - 0.001), (L, ztop(L) - 0.004), (L - 0.010, ztop(L) - 0.020), (0.70, ztop(0.70) - 0.036)] + \
       [(y, ztop(y) - 0.038) for y in reversed(ys) if y < 0.69]
EDGEPTS = edge
P.append(prof('edge', edge, 0.0040, JADEM, bev=0.0008, segs=1, taper=0.3))
# habaki + seppa + tsuba disc
P.append(prof('habaki', rrect(-0.004, -0.024, 0.032, ztop(0) + 0.004, 0.003, 2), 0.013, GOLD, bev=0.001))
P.append(lathe('seppa', [(0.0, -0.024), (0.022, -0.024), (0.022, -0.004), (0.0, -0.004)], DARK, 20, at=(0, 0, 0.0), bev=0.0008))
DISC = []
DISC.append(lathe('discring', [(0.050, -0.019), (0.064, -0.018), (0.066, -0.013), (0.064, -0.008), (0.050, -0.007)], JADEM, 32, at=(0, 0, 0), bev=0, ngon_caps=False))
DISC.append(lathe('discin', [(0.016, -0.017), (0.051, -0.017), (0.051, -0.009), (0.016, -0.009)], DARK, 32, at=(0, 0, 0), bev=0.0008, ngon_caps=False))
DISC.append(lathe('discgold', [(0.044, -0.0185), (0.0475, -0.0185), (0.0475, -0.0075), (0.044, -0.0075)], GOLD, 32, at=(0, 0, 0), bev=0, ngon_caps=False))
dc = DISC[1]
cut(dc, [cutter_box((0.02, 0.03, 0.012), (0.0, -0.013, 0.0)) for _ in range(0)])
for a in (45, 135, 225, 315):
    sp = box('spoke', (0.005, 0.009, 0.030), (0, -0.013, 0.031), DARK, 0.0006)
    sp.data.transform(Matrix.Rotation(radians(a), 4, 'Y')); DISC.append(sp)
DISCO = join(DISC, f'{TAG}_Disc')
# tsuka (handle) - elliptical, wrapped
# straight, constant cross-section (no taper or swell), centred on the blade axis (x = z = 0)
tsuka = lathe('tsuka', [(0.0, TS0), (TSR, TS0), (TSR, -0.026), (0.0, -0.026)], WRAP, 20, at=(0, 0, 0), bev=0)
tsuka.data.transform(Matrix.Diagonal((TSX, 1, 1, 1)))
P.append(tsuka)
P.append(lathe('fuchi', [(0.0, -0.034), (0.0185, -0.034), (0.0195, -0.026), (0.0195, -0.023), (0.0, -0.023)], DARK, 16, at=(0, 0, 0), bev=0.0006))
P.append(lathe('fuchigold', [(0.0188, -0.038), (0.0192, -0.038), (0.0192, -0.034), (0.0188, -0.034)], GOLD, 16, at=(0, 0, 0), bev=0, ngon_caps=False))
P.append(lathe('kashira', [(0.0, -0.335), (0.0145, -0.334), (0.0175, -0.326), (0.0175, -0.300), (0.0, -0.300)], DARK, 16, at=(0, 0, 0), bev=0.0008))
P.append(lathe('kashiragold', [(0.0176, -0.316), (0.0184, -0.316), (0.0184, -0.310), (0.0176, -0.310)], GOLD, 16, at=(0, 0, 0), bev=0, ngon_caps=False))

from collections import Counter
_c = Counter()
for o in P: _c[o.name.split('.')[0]] += tris(o)
log('PARTS', _c.most_common(12))
BODY = join(P, f'{TAG}_Body')
FX_EDGE = prof(f'{TAG}_EdgeFX', EDGEPTS, 0.0046, GLOW, bev=0)
FX_DISC = lathe(f'{TAG}_DiscFX', [(0.0645, -0.0175), (0.0675, -0.0168), (0.0675, -0.0092), (0.0645, -0.0085)], GLOW, 32, at=(0, 0, 0), bev=0, ngon_caps=False)
MESHES = [BODY, DISCO]; FX = [FX_EDGE, FX_DISC]
for o in MESHES + FX:
    o.data.transform(Matrix.Translation(-G))
root = empty(TAG); pivot = empty(f'{TAG}_Pivot', parent=root)
def piv(o, p):
    set_origin(o, Vector(p) - G); o.parent = pivot
piv(BODY, G); piv(DISCO, (0, -0.013, 0)); piv(FX_DISC, (0, -0.013, 0)); piv(FX_EDGE, (0, 0.35, ztop(0.35) - 0.026))
SOCK = {'Tip': (0, L, ztop(L) - 0.004), 'BladeBase': (0, 0.03, 0.0), 'OffHand': (0, -0.27, 0.0), 'TrailStart': (0, 0.10, 0.0), 'TrailEnd': (0, 0.72, 0.03)}
for k, v in SOCK.items():
    empty(f'{TAG}_Socket_{k}', Vector(v) - G, pivot, 0.01)
animated(pivot, DISCO, FX_EDGE, FX_DISC)
clip('Idle', 60, {pivot: [(30, {'l': (0, 0, -0.004), 'r': (0.8, 0.6, 0.4)})], FX_DISC: [(30, {'s': (1.05, 1, 1.05)})], FX_EDGE: [(30, {'s': (1.25, 1, 1.0)})]},
     loop=True, desc='breathing sway + jade glow pulse')
clip('Equip', 24, {pivot: [(0, {'l': (0, -0.35, -0.05), 'r': (0, 0, 0)}), (8, {'l': (0, -0.05, 0.0), 'r': (0, 0, 0)}), (14, {'l': (0, 0.02, 0.02), 'r': (8, -20, 6)}), (24, {})],
                   FX_EDGE: [(0, {'s': (0.2, 1, 1)}), (8, {'s': (1.8, 1, 1)}), (16, {})], FX_DISC: [(6, {}), (10, {'s': (1.15, 1, 1.15)}), (16, {})]},
     desc='quick-draw along the blade axis (as if from a saya) then flourish')
clip('SwingLight', 18, {pivot: [(0, {}), (4, {'l': (0.03, -0.03, 0.02), 'r': (5, -25, 45)}), (9, {'l': (-0.06, 0.04, 0.0), 'r': (-5, 35, -70)}), (13, {'l': (-0.04, 0.02, 0), 'r': (-3, 25, -55)}), (18, {})],
                        FX_EDGE: [(4, {}), (7, {'s': (2.2, 1, 1)}), (11, {'s': (1.6, 1, 1)}), (16, {})]},
     desc='fast horizontal slash right-to-left; use Socket_TrailStart/TrailEnd for the trail ribbon; hit window f6-f10')
clip('SwingHeavy', 36, {pivot: [(0, {}), (10, {'l': (0, -0.06, 0.12), 'r': (65, -10, 10)}), (16, {'l': (0, -0.06, 0.14), 'r': (72, -12, 12)}),
                                (22, {'l': (0, 0.06, -0.08), 'r': (-55, 5, -5)}), (27, {'l': (0, 0.05, -0.07), 'r': (-48, 4, -4)}), (36, {})],
                        FX_EDGE: [(10, {}), (16, {'s': (1.6, 1, 1)}), (20, {'s': (2.6, 1, 1)}), (26, {'s': (1.8, 1, 1)}), (34, {})],
                        FX_DISC: [(16, {}), (20, {'s': (1.12, 1, 1.12)}), (28, {})]},
     desc='overhead wind-up and downward cleave; hit window f19-f24')
clip('Parry', 24, {pivot: [(0, {}), (5, {'l': (0, 0.02, 0.10), 'r': (10, -80, 0)}), (16, {'l': (0, 0.02, 0.10), 'r': (8, -82, 0)}), (24, {})],
                   DISCO: [(4, {}), (12, {'r': (0, 180, 0)}), (24, {'r': (0, 360, 0)})],
                   FX_DISC: [(4, {}), (6, {'s': (1.6, 2.5, 1.6)}), (9, {'s': (1.3, 1.8, 1.3)}), (16, {'s': (1.08, 1, 1.08)}), (24, {})],
                   FX_EDGE: [(5, {}), (7, {'s': (1.8, 1, 1)}), (16, {})]},
     desc='blade raised flat across the body; the parry disc flares and spins (block window f5-f16)')
clip('Inspect', 120, {pivot: [(20, {'l': (0, 0.0, 0.05), 'r': (0, 0, -60)}), (40, {'l': (0, 0.0, 0.05), 'r': (0, 180, -60)}), (60, {'l': (0, 0.0, 0.05), 'r': (0, 360, -60)}),
                              (80, {'l': (0, 0.0, 0.05), 'r': (25, 360, -30)}), (100, {'r': (5, 360, 0)}), (120, {'r': (0, 360, 0)})],
                      DISCO: [(60, {}), (90, {'r': (0, 180, 0)}), (120, {'r': (0, 360, 0)})],
                      FX_DISC: [(80, {}), (88, {'s': (1.12, 1, 1.12)}), (96, {})]}, desc='turn the blade edge-on, full twist to show both faces, disc spin')
run_pipeline(W, TAG, root, MESHES, FX, ['Idle', 'SwingLight', 'SwingHeavy', 'Parry', 'Inspect'], accent=(0.3, 1.0, 0.6), cam_dir=(-1.0, 0.30, 0.42), lens=70)
