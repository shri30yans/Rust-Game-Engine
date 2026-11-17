"""Build the long-slide tactical pistol in Blender with 1:1 pixel-traced profiles.

Follows docs/CONTENT.md conventions:
- Units: metres (1 unit = 1 m), forward is -Y, up is +Z, right is +X.
- Origin / attachment point at grip (`socket_grip`).
- Separate movable objects (`frame`, `slide`, `barrel`, `trigger`, `magazine`,
  `grip_panels`, `sights`, `controls`) with applied rotation and scale.
- Standard weapon sockets: `socket_grip`, `socket_muzzle`, `socket_eject`,
  `socket_sight`, `socket_mag`.
"""

from __future__ import annotations

import argparse
import json
import math
import shutil
import struct
import sys
from pathlib import Path

import bmesh
import bpy
from mathutils import Euler, Matrix, Vector
from mathutils.geometry import tessellate_polygon


# Pixel-to-meter mapping from the 400x200 reference image:
# In the 400x200 reference image, x goes 0..400 (left-to-right, rear-to-muzzle)
# and y goes 0..200 (bottom-to-top).
# In Blender, -Y is forward (right in side view) and +Z is up.
PX_SCALE = 0.00085  # 0.85 mm per pixel -> ~272 mm total length
PX_X0 = 100.0
PX_Y0 = 100.0


def px(x: float, y: float) -> tuple[float, float]:
    """Convert (x, y) pixel coordinates from the 400x200 reference sprite to Blender (Y, Z) metres."""
    return ((PX_X0 - x) * PX_SCALE, (y - PX_Y0) * PX_SCALE)


def px_len(val: float) -> float:
    """Convert a pixel length to metres."""
    return val * PX_SCALE


def copy_if_different(source: Path, destination: Path) -> None:
    """Copy generated content unless Blender wrote directly to its batch destination."""
    if source.resolve() != destination.resolve():
        shutil.copy2(source, destination)


COLOR_PRESETS: dict[str, dict[str, tuple[float, float, float, float]]] = {
    "midnight": {
        # Matches media_1794410276759_046ffa5f.png:
        # Deep blued navy-charcoal slide with bright silver-white top bevel,
        # rich dark midnight navy-slate frame, jet-black diamond grip panel,
        # and warm bronze-gunmetal chamber hood & muzzle.
        "slide_dark": (0.008, 0.012, 0.022, 1.0),
        "slide_mid": (0.016, 0.024, 0.042, 1.0),
        "slide_light": (0.680, 0.720, 0.800, 1.0),
        "frame_dark": (0.012, 0.016, 0.028, 1.0),
        "frame_light": (0.025, 0.033, 0.055, 1.0),
        "grip_light": (0.018, 0.020, 0.026, 1.0),
        "grip_dark": (0.002, 0.002, 0.004, 1.0),
        "barrel": (0.130, 0.108, 0.082, 1.0),
    },
    "bronze": {
        # Matches media_1794404052273_67acfea2.png:
        # Warm brushed bronze-gold slide & grip panel with matte black frame.
        "slide_dark": (0.140, 0.095, 0.042, 1.0),
        "slide_mid": (0.290, 0.200, 0.092, 1.0),
        "slide_light": (0.780, 0.620, 0.340, 1.0),
        "frame_dark": (0.010, 0.011, 0.014, 1.0),
        "frame_light": (0.025, 0.027, 0.032, 1.0),
        "grip_light": (0.420, 0.300, 0.135, 1.0),
        "grip_dark": (0.045, 0.032, 0.012, 1.0),
        "barrel": (0.042, 0.046, 0.054, 1.0),
    },
    "crimson": {
        "slide_dark": (0.060, 0.008, 0.014, 1.0),
        "slide_mid": (0.240, 0.018, 0.034, 1.0),
        "slide_light": (0.760, 0.120, 0.160, 1.0),
        "frame_dark": (0.010, 0.011, 0.014, 1.0),
        "frame_light": (0.028, 0.030, 0.036, 1.0),
        "grip_light": (0.180, 0.018, 0.032, 1.0),
        "grip_dark": (0.025, 0.004, 0.009, 1.0),
        "barrel": (0.055, 0.058, 0.066, 1.0),
    },
}


def parse_args() -> argparse.Namespace:
    argv = sys.argv
    if "--" in argv:
        argv = argv[argv.index("--") + 1 :]
    else:
        argv = []
    parser = argparse.ArgumentParser(description="Generate the tactical pistol.")
    parser.add_argument(
        "--color",
        choices=list(COLOR_PRESETS.keys()),
        default="midnight",
        help="Colorway preset",
    )
    parser.add_argument(
        "--blend",
        type=Path,
        default=Path("games/imagine/content/blender/pistol.blend"),
        help="Output .blend path",
    )
    parser.add_argument(
        "--glb",
        type=Path,
        default=Path("games/imagine/content/exported/pistol.glb"),
        help="Output .glb path",
    )
    parser.add_argument(
        "--render",
        type=Path,
        default=None,
        help="Optional output PNG path for side-profile render",
    )
    parser.add_argument(
        "--render-persp",
        type=Path,
        default=None,
        help="Optional output PNG path for 3/4 perspective render",
    )
    parser.add_argument(
        "--batch-root",
        type=Path,
        default=Path("games/imagine/content/weapons_batch1"),
        help="Path to weapons_batch1 root to populate export/, renders/, and blender/pistol/",
    )
    return parser.parse_args(argv)


def reset_scene() -> None:
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.render.fps = 30
    scene.unit_settings.system = "METRIC"
    scene.unit_settings.scale_length = 1.0


def set_bsdf_input(bsdf: bpy.types.Node, names: list[str], value: object) -> None:
    for name in names:
        if name in bsdf.inputs:
            bsdf.inputs[name].default_value = value
            return


def make_materials(color_name: str) -> dict[str, bpy.types.Material]:
    palette = COLOR_PRESETS[color_name]
    mats: dict[str, bpy.types.Material] = {}

    # 1. Brushed Slide Material (dark flat side + bright specular top chamfer)
    mat_slide = bpy.data.materials.new(name="steel_slide")
    nt = mat_slide.node_tree
    bsdf = nt.nodes.get("Principled BSDF")
    tex_coord = nt.nodes.new("ShaderNodeTexCoord")

    sep_xyz = nt.nodes.new("ShaderNodeSeparateXYZ")
    map_range = nt.nodes.new("ShaderNodeMapRange")
    _, z_slide_bot = px(0, 145.0)
    _, z_slide_top = px(0, 165.0)
    map_range.inputs["From Min"].default_value = z_slide_bot
    map_range.inputs["From Max"].default_value = z_slide_top
    map_range.inputs["To Min"].default_value = 0.0
    map_range.inputs["To Max"].default_value = 1.0

    mapping = nt.nodes.new("ShaderNodeMapping")
    mapping.inputs["Scale"].default_value = (1.0, 340.0, 18.0)
    noise = nt.nodes.new("ShaderNodeTexNoise")
    noise.inputs["Scale"].default_value = 70.0
    noise.inputs["Detail"].default_value = 10.0
    noise.inputs["Roughness"].default_value = 0.65

    mix_fac = nt.nodes.new("ShaderNodeMix")
    mix_fac.data_type = "FLOAT"
    mix_fac.inputs["Factor"].default_value = 0.18

    ramp = nt.nodes.new("ShaderNodeValToRGB")
    ramp.color_ramp.elements[0].position = 0.15
    ramp.color_ramp.elements[0].color = palette["slide_dark"]
    el_mid = ramp.color_ramp.elements.new(0.68)
    el_mid.color = palette["slide_mid"]
    ramp.color_ramp.elements[2].position = 0.86
    ramp.color_ramp.elements[2].color = palette["slide_light"]

    bump = nt.nodes.new("ShaderNodeBump")
    bump.inputs["Strength"].default_value = 0.06
    bump.inputs["Distance"].default_value = 0.0004

    nt.links.new(tex_coord.outputs["Object"], sep_xyz.inputs["Vector"])
    nt.links.new(sep_xyz.outputs["Z"], map_range.inputs["Value"])
    nt.links.new(tex_coord.outputs["Object"], mapping.inputs["Vector"])
    nt.links.new(mapping.outputs["Vector"], noise.inputs["Vector"])
    nt.links.new(map_range.outputs["Result"], mix_fac.inputs["A"])
    nt.links.new(noise.outputs["Fac"], mix_fac.inputs["B"])
    nt.links.new(mix_fac.outputs["Result"], ramp.inputs["Fac"])
    nt.links.new(ramp.outputs["Color"], bsdf.inputs["Base Color"])
    nt.links.new(noise.outputs["Fac"], bump.inputs["Height"])
    nt.links.new(bump.outputs["Normal"], bsdf.inputs["Normal"])

    set_bsdf_input(bsdf, ["Metallic"], 0.88)
    set_bsdf_input(bsdf, ["Roughness"], 0.25)
    mats["slide"] = mat_slide

    # 2. Matte Textured Lower Frame Material
    mat_frame = bpy.data.materials.new(name="polymer_frame")
    nt = mat_frame.node_tree
    bsdf = nt.nodes.get("Principled BSDF")
    tex_coord = nt.nodes.new("ShaderNodeTexCoord")
    noise = nt.nodes.new("ShaderNodeTexNoise")
    noise.inputs["Scale"].default_value = 850.0
    noise.inputs["Detail"].default_value = 12.0
    noise.inputs["Roughness"].default_value = 0.80

    ramp = nt.nodes.new("ShaderNodeValToRGB")
    ramp.color_ramp.elements[0].position = 0.20
    ramp.color_ramp.elements[0].color = palette["frame_dark"]
    ramp.color_ramp.elements[1].position = 0.85
    ramp.color_ramp.elements[1].color = palette["frame_light"]

    bump = nt.nodes.new("ShaderNodeBump")
    bump.inputs["Strength"].default_value = 0.20
    bump.inputs["Distance"].default_value = 0.0008

    nt.links.new(tex_coord.outputs["Object"], noise.inputs["Vector"])
    nt.links.new(noise.outputs["Fac"], ramp.inputs["Fac"])
    nt.links.new(ramp.outputs["Color"], bsdf.inputs["Base Color"])
    nt.links.new(noise.outputs["Fac"], bump.inputs["Height"])
    nt.links.new(bump.outputs["Normal"], bsdf.inputs["Normal"])

    set_bsdf_input(bsdf, ["Metallic"], 0.25)
    set_bsdf_input(bsdf, ["Roughness"], 0.46)
    mats["frame"] = mat_frame

    # 3. Checkered Diamond Grip Inlay Material
    mat_grip = bpy.data.materials.new(name="checkered_grip")
    nt = mat_grip.node_tree
    bsdf = nt.nodes.get("Principled BSDF")
    tex_coord = nt.nodes.new("ShaderNodeTexCoord")
    mapping = nt.nodes.new("ShaderNodeMapping")
    mapping.inputs["Rotation"].default_value = (0.46, 0.0, 0.0)
    mapping.inputs["Scale"].default_value = (155.0, 155.0, 155.0)

    checker = nt.nodes.new("ShaderNodeTexVoronoi")
    checker.feature = "F1"
    checker.distance = "MANHATTAN"
    checker.inputs["Scale"].default_value = 3.5
    checker.inputs["Randomness"].default_value = 0.0

    ramp = nt.nodes.new("ShaderNodeValToRGB")
    ramp.color_ramp.elements[0].position = 0.05
    ramp.color_ramp.elements[0].color = palette["grip_light"]
    ramp.color_ramp.elements[1].position = 0.62
    ramp.color_ramp.elements[1].color = palette["grip_dark"]

    bump = nt.nodes.new("ShaderNodeBump")
    bump.invert = True
    bump.inputs["Strength"].default_value = 0.65
    bump.inputs["Distance"].default_value = 0.0012

    nt.links.new(tex_coord.outputs["Object"], mapping.inputs["Vector"])
    nt.links.new(mapping.outputs["Vector"], checker.inputs["Vector"])
    nt.links.new(checker.outputs["Distance"], ramp.inputs["Fac"])
    nt.links.new(ramp.outputs["Color"], bsdf.inputs["Base Color"])
    nt.links.new(checker.outputs["Distance"], bump.inputs["Height"])
    nt.links.new(bump.outputs["Normal"], bsdf.inputs["Normal"])

    set_bsdf_input(bsdf, ["Metallic"], 0.35)
    set_bsdf_input(bsdf, ["Roughness"], 0.34)
    mats["grip"] = mat_grip

    # 4. Warm Bronze-Steel Barrel & Muzzle Material
    mat_barrel = bpy.data.materials.new(name="steel_barrel")
    bsdf = mat_barrel.node_tree.nodes.get("Principled BSDF")
    set_bsdf_input(bsdf, ["Base Color"], palette["barrel"])
    set_bsdf_input(bsdf, ["Metallic"], 0.88)
    set_bsdf_input(bsdf, ["Roughness"], 0.25)
    mats["barrel"] = mat_barrel

    # 5. Dark Gunmetal Steel (Trigger, Sights, Controls, Screws)
    mat_steel = bpy.data.materials.new(name="steel_gunmetal")
    bsdf = mat_steel.node_tree.nodes.get("Principled BSDF")
    set_bsdf_input(bsdf, ["Base Color"], (0.018, 0.020, 0.026, 1.0))
    set_bsdf_input(bsdf, ["Metallic"], 0.88)
    set_bsdf_input(bsdf, ["Roughness"], 0.26)
    mats["steel"] = mat_steel

    # 6. Polished Stainless Accent (Magazine Baseplate Rim & Chamber Roof)
    mat_chrome = bpy.data.materials.new(name="steel_polished_rim")
    bsdf = mat_chrome.node_tree.nodes.get("Principled BSDF")
    set_bsdf_input(bsdf, ["Base Color"], (0.58, 0.60, 0.64, 1.0))
    set_bsdf_input(bsdf, ["Metallic"], 0.95)
    set_bsdf_input(bsdf, ["Roughness"], 0.16)
    mats["chrome"] = mat_chrome

    return mats


def create_mesh_object(name: str, bm: bmesh.types.BMesh, mat: bpy.types.Material) -> bpy.types.Object:
    mesh = bpy.data.meshes.new(name)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.normal_update()
    bm.to_mesh(mesh)
    bm.free()
    for poly in mesh.polygons:
        poly.use_smooth = True
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.collection.objects.link(obj)
    obj.data.materials.append(mat)
    return obj


def add_edge_split_sharp(obj: bpy.types.Object, angle_deg: float = 34.0) -> None:
    mod = obj.modifiers.new(name="EdgeSplit", type="EDGE_SPLIT")
    mod.split_angle = math.radians(angle_deg)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.modifier_apply(modifier=mod.name)


def extrude_2d_profile_x(
    name: str,
    profile_yz: list[tuple[float, float]],
    half_width: float,
    mat: bpy.types.Material,
    bevel_width: float = 0.0010,
    bevel_segments: int = 2,
) -> bpy.types.Object:
    """Create a closed manifold 3D mesh from a 2D (Y, Z) polygon extruded along X."""
    bm = bmesh.new()
    verts_neg = [bm.verts.new((-half_width, y, z)) for (y, z) in profile_yz]
    verts_pos = [bm.verts.new((half_width, y, z)) for (y, z) in profile_yz]

    loop_2d = [Vector((y, z, 0.0)) for (y, z) in profile_yz]
    tris = tessellate_polygon([loop_2d])
    for i0, i1, i2 in tris:
        bm.faces.new((verts_neg[i0], verts_neg[i2], verts_neg[i1]))
        bm.faces.new((verts_pos[i0], verts_pos[i1], verts_pos[i2]))

    n = len(profile_yz)
    for i in range(n):
        j = (i + 1) % n
        bm.faces.new((verts_neg[i], verts_neg[j], verts_pos[j], verts_pos[i]))

    obj = create_mesh_object(name, bm, mat)

    if bevel_width > 0.0:
        bev = obj.modifiers.new(name="Bevel", type="BEVEL")
        bev.width = bevel_width
        bev.segments = bevel_segments
        bev.limit_method = "ANGLE"
        bev.angle_limit = math.radians(38.0)
        bpy.context.view_layer.objects.active = obj
        bpy.ops.object.modifier_apply(modifier=bev.name)

    return obj


def boolean_op(target: bpy.types.Object, cutter: bpy.types.Object, operation: str = "DIFFERENCE") -> None:
    mod = target.modifiers.new(name=f"Bool_{cutter.name}", type="BOOLEAN")
    mod.operation = operation
    mod.solver = "FLOAT"
    mod.object = cutter
    bpy.context.view_layer.objects.active = target
    bpy.ops.object.modifier_apply(modifier=mod.name)
    bpy.data.objects.remove(cutter, do_unlink=True)
    if len(target.data.polygons) == 0:
        raise RuntimeError(f"Boolean on '{target.name}' resulted in an empty mesh")


def build_slide(mats: dict[str, bpy.types.Material]) -> bpy.types.Object:
    """Build the upper slide matching pixel rows y=144.8..165.0 and x=61.5..342.5."""
    y_rear, z_bot = px(61.5, 144.8)
    y_front, _ = px(342.5, 144.8)
    _, z_mid = px(0, 159.5)
    _, z_top = px(0, 165.0)
    w_half = 0.0136
    w_top = 0.0092

    bm = bmesh.new()
    cross_xz = [
        (-w_half, z_bot),
        (w_half, z_bot),
        (w_half, z_mid),
        (w_top, z_top),
        (-w_top, z_top),
        (-w_half, z_mid),
    ]
    v_front = [bm.verts.new((x, y_front, z)) for (x, z) in cross_xz]
    v_rear = [bm.verts.new((x, y_rear, z)) for (x, z) in cross_xz]
    # Top rear of slide slants forward by 3.5 px (from x=61.5 at y=145 to x=65.0 at y=165)
    for v in v_rear:
        if v.co.z > z_mid - 0.001:
            v.co.y -= px_len(3.5)
    for v in v_front:
        if v.co.z > z_mid - 0.001:
            v.co.y += px_len(1.8)

    bm.faces.new(v_front)
    bm.faces.new(list(reversed(v_rear)))
    n = len(cross_xz)
    for i in range(n):
        j = (i + 1) % n
        bm.faces.new([v_front[i], v_front[j], v_rear[j], v_rear[i]])

    slide = create_mesh_object("slide", bm, mats["slide"])

    # 1. Ejection port cutout: x=130..168.5, y=151.2..166.0
    port_y, port_z = px((130.0 + 168.5) * 0.5, (151.2 + 166.5) * 0.5)
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=(0.0, port_y, port_z))
    port = bpy.context.active_object
    port.scale = (0.030, px_len(38.5), px_len(15.3))
    bpy.ops.object.transform_apply(scale=True)
    bev = port.modifiers.new(name="Bevel", type="BEVEL")
    bev.width = 0.0024
    bev.segments = 4
    bpy.ops.object.modifier_apply(modifier=bev.name)
    boolean_op(slide, port, "DIFFERENCE")

    # 2. Extractor recess behind ejection port: x=109..130, y=152.5..157.5
    ext_y, ext_z = px((109.0 + 130.0) * 0.5, 155.0)
    for side in (-1.0, 1.0):
        bpy.ops.mesh.primitive_cube_add(size=1.0, location=(side * w_half, ext_y, ext_z))
        ext_cut = bpy.context.active_object
        ext_cut.scale = (0.0032, px_len(21.0), px_len(5.0))
        bpy.ops.object.transform_apply(scale=True)
        boolean_op(slide, ext_cut, "DIFFERENCE")

    # 3. 7 angled rear cocking serrations: x=71..101, y=145.5..160.5
    for idx in range(7):
        px_x = 72.0 + idx * 4.6
        serr_y, serr_z = px(px_x, 153.0)
        for side in (-1.0, 1.0):
            bpy.ops.mesh.primitive_cube_add(
                size=1.0,
                location=(side * (w_half - 0.0002), serr_y, serr_z),
                rotation=(math.radians(-8.5), 0.0, 0.0),
            )
            cutter = bpy.context.active_object
            cutter.scale = (0.0032, px_len(2.4), px_len(15.2))
            bpy.ops.object.transform_apply(rotation=True, scale=True)
            boolean_op(slide, cutter, "DIFFERENCE")

    # 4. Small vertical notch near front bottom of slide: x=332, y=146..151
    notch_y, notch_z = px(332.0, 148.5)
    for side in (-1.0, 1.0):
        bpy.ops.mesh.primitive_cube_add(
            size=1.0,
            location=(side * (w_half - 0.0001), notch_y, notch_z),
        )
        notch = bpy.context.active_object
        notch.scale = (0.0022, px_len(2.5), px_len(5.5))
        bpy.ops.object.transform_apply(scale=True)
        boolean_op(slide, notch, "DIFFERENCE")

    add_edge_split_sharp(slide, 32.0)
    return slide


def build_barrel_and_muzzle(mats: dict[str, bpy.types.Material]) -> bpy.types.Object:
    """Build the chamber hood (x=130..168.5, y=151..164.5) and protruding muzzle (x=342.5..359.5, y=141..164)."""
    parts: list[bpy.types.Object] = []

    hood_y, hood_z = px((129.5 + 168.5) * 0.5, (150.5 + 164.2) * 0.5)
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=(0.0, hood_y, hood_z))
    hood = bpy.context.active_object
    hood.name = "barrel_hood"
    hood.scale = (0.0254, px_len(39.0), px_len(13.7))
    bpy.ops.object.transform_apply(scale=True)
    bev = hood.modifiers.new(name="Bevel", type="BEVEL")
    bev.width = 0.0020
    bev.segments = 3
    bpy.ops.object.modifier_apply(modifier=bev.name)

    # Cartridge stamp recess on side of chamber hood
    stamp_y, stamp_z = px(144.0, 154.0)
    for side in (-1.0, 1.0):
        bpy.ops.mesh.primitive_cube_add(size=1.0, location=(side * 0.0126, stamp_y, stamp_z))
        stamp = bpy.context.active_object
        stamp.scale = (0.0014, px_len(19.0), px_len(3.8))
        bpy.ops.object.transform_apply(scale=True)
        boolean_op(hood, stamp, "DIFFERENCE")

    hood.data.materials.append(mats["barrel"])
    parts.append(hood)

    # Bright silver top highlight strip on chamber hood roof (y=162..164.5)
    top_y, top_z = px((131.0 + 167.0) * 0.5, 163.5)
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=(0.0, top_y, top_z))
    hood_top = bpy.context.active_object
    hood_top.scale = (0.019, px_len(35.0), px_len(2.2))
    bpy.ops.object.transform_apply(scale=True)
    hood_top.data.materials.append(mats["chrome"])
    parts.append(hood_top)

    # Extractor bar: x=109.5..129.5, y=155.0
    ext_y, ext_z = px(119.5, 155.0)
    for side in (-1.0, 1.0):
        bpy.ops.mesh.primitive_cube_add(size=1.0, location=(side * 0.0128, ext_y, ext_z))
        ext = bpy.context.active_object
        ext.scale = (0.0022, px_len(19.5), px_len(4.2))
        bpy.ops.object.transform_apply(scale=True)
        ext.data.materials.append(mats["steel"])
        parts.append(ext)

    # Protruding muzzle cylinder: x=342.5..359.5, centered at y=152.5, radius=11.2 px
    muz_y, muz_z = px(349.5, 152.5)
    bpy.ops.mesh.primitive_cylinder_add(
        vertices=36,
        radius=px_len(11.2),
        depth=px_len(22.0),
        location=(0.0, muz_y, muz_z),
        rotation=(math.radians(90.0), 0.0, 0.0),
    )
    tube = bpy.context.active_object
    bpy.ops.object.transform_apply(rotation=True)

    bev_m = tube.modifiers.new(name="Bevel", type="BEVEL")
    bev_m.width = 0.0018
    bev_m.segments = 3
    bpy.context.view_layer.objects.active = tube
    bpy.ops.object.modifier_apply(modifier=bev_m.name)

    bore_y, _ = px(356.0, 152.5)
    bpy.ops.mesh.primitive_cylinder_add(
        vertices=28,
        radius=px_len(6.5),
        depth=px_len(14.0),
        location=(0.0, bore_y, muz_z),
        rotation=(math.radians(90.0), 0.0, 0.0),
    )
    bore = bpy.context.active_object
    bpy.ops.object.transform_apply(rotation=True)
    boolean_op(tube, bore, "DIFFERENCE")

    slot_y, _ = px(353.0, 152.5)
    for side in (-1.0, 1.0):
        bpy.ops.mesh.primitive_cube_add(
            size=1.0,
            location=(side * 0.0082, slot_y, muz_z),
        )
        slot = bpy.context.active_object
        slot.scale = (0.0036, px_len(4.2), px_len(15.5))
        bpy.ops.object.transform_apply(scale=True)
        boolean_op(tube, slot, "DIFFERENCE")

    tube.data.materials.append(mats["barrel"])
    parts.append(tube)

    bpy.ops.object.select_all(action="DESELECT")
    for p in parts:
        p.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.join()
    barrel = bpy.context.active_object
    barrel.name = "barrel"
    add_edge_split_sharp(barrel, 34.0)
    return barrel


def build_sights(mats: dict[str, bpy.types.Material]) -> bpy.types.Object:
    """Build the rear sight (x=67..92, y=164.5..171.5) and front sight (x=315..329.5, y=164.5..171.5)."""
    rear_profile = [
        px(92.0, 164.5),
        px(67.0, 164.5),
        px(67.5, 171.5),
        px(75.0, 171.5),
        px(81.0, 168.5),
        px(92.0, 168.0),
    ]
    rear = extrude_2d_profile_x("sight_rear", rear_profile, 0.0080, mats["steel"], bevel_width=0.0005)

    front_profile = [
        px(329.5, 164.5),
        px(315.0, 164.5),
        px(317.0, 171.2),
        px(327.0, 171.5),
    ]
    front = extrude_2d_profile_x("sight_front", front_profile, 0.0024, mats["steel"], bevel_width=0.0004)

    bpy.ops.object.select_all(action="DESELECT")
    rear.select_set(True)
    front.select_set(True)
    bpy.context.view_layer.objects.active = rear
    bpy.ops.object.join()
    rear.name = "sights"
    add_edge_split_sharp(rear, 34.0)
    return rear


def build_frame(mats: dict[str, bpy.types.Material]) -> bpy.types.Object:
    """Build the lower frame, dust cover, protruding lower rail ridge, trigger guard, and tapered grip."""
    # Exact outer silhouette from pixel measurements of the 400x200 reference image:
    frame_outer_px = [
        (342.5, 144.8),  # Front top of dust cover under slide
        (342.5, 108.0),  # Front face of dust cover
        (340.5, 97.2),   # Front bottom corner of rail
        (200.0, 97.2),   # Bottom of underbarrel rail
        (194.0, 95.0),   # Transition to trigger guard front
        (189.0, 91.0),   # Trigger guard front-bottom curve
        (182.0, 86.8),   # Trigger guard bottom-right curve
        (173.0, 85.2),   # Trigger guard bottom flat start
        (138.0, 85.2),   # Trigger guard bottom flat end
        (127.0, 88.2),   # Trigger guard root rise
        (119.5, 90.2),   # High undercut peak behind trigger guard
        (113.5, 87.5),   # Front-strap top corner under undercut
        (110.5, 80.5),   # Upper front strap
        (106.0, 69.5),   # Upper-mid front strap
        (101.8, 57.5),   # Mid-lower front strap
        (98.5, 45.5),    # Lower front strap
        (96.0, 35.0),    # Tapered bottom-front of grip
        (94.2, 31.8),    # Bottom-front toe above baseplate
        (47.2, 32.5),    # Bottom-rear heel above baseplate
        (45.5, 38.5),    # Lower-rear curve
        (47.2, 48.5),    # Lower backstrap
        (51.0, 60.5),    # Mid backstrap
        (56.0, 73.5),    # Upper-mid backstrap
        (61.5, 86.5),    # Upper backstrap
        (66.5, 98.5),    # Thumb web lower curve
        (70.2, 108.0),   # Thumb web deepest point
        (70.5, 114.5),   # Thumb web upper curve
        (67.5, 120.5),   # Under tang curve
        (62.5, 123.5),   # Inner corner of rear tang
        (52.5, 124.8),   # Squared rear tang overhang bottom-left
        (52.0, 129.0),   # Squared rear tang left tip
        (56.0, 136.5),   # Rear frame slant up toward slide
        (61.5, 144.8),   # Rear top of frame under slide
    ]
    frame_profile = [px(x, y) for (x, y) in frame_outer_px]
    body = extrude_2d_profile_x(
        "frame_body",
        frame_profile,
        half_width=0.0142,
        mat=mats["frame"],
        bevel_width=0.0018,
        bevel_segments=3,
    )

    # Cut out the trigger guard opening (`tg_inner` from pixel hole measurements)
    tg_inner_px = [
        (129.5, 114.5),
        (135.0, 118.2),
        (154.0, 119.2),
        (171.0, 119.2),
        (178.5, 116.8),
        (183.2, 111.5),
        (183.8, 102.0),
        (180.5, 94.5),
        (174.0, 90.5),
        (138.0, 90.5),
        (131.5, 93.5),
        (128.0, 99.5),
        (127.5, 108.5),
    ]
    tg_cut = extrude_2d_profile_x(
        "tg_cutter",
        [px(x, y) for (x, y) in tg_inner_px],
        half_width=0.020,
        mat=mats["frame"],
        bevel_width=0.0,
    )
    boolean_op(body, tg_cut, "DIFFERENCE")

    # Narrow the trigger guard strap in X (from +/-0.0142 to +/-0.0068) so it looks sleek in 3/4 view
    tg_trim_y, tg_trim_z = px(156.0, 98.0)
    for side in (-1.0, 1.0):
        bpy.ops.mesh.primitive_cube_add(
            size=1.0,
            location=(side * 0.0138, tg_trim_y, tg_trim_z),
        )
        side_trim = bpy.context.active_object
        side_trim.scale = (0.014, px_len(72.0), px_len(34.0))
        bpy.ops.object.transform_apply(scale=True)
        boolean_op(body, side_trim, "DIFFERENCE")

    # Protruding lower rail ridge along the dust cover (x=193..342.8, y=97.2..111.5)
    # In the reference image, this ridge steps slightly outward and has a 45° top bevel at y=108..111.5
    # that catches a bright horizontal highlight line right below a dark groove at y=113..115!
    rail_px = [
        (342.8, 111.0),
        (342.8, 99.0),
        (340.5, 97.2),
        (197.0, 97.2),
        (192.0, 102.0),
        (194.5, 111.0),
    ]
    rail = extrude_2d_profile_x(
        "frame_rail",
        [px(x, y) for (x, y) in rail_px],
        half_width=0.0154,
        mat=mats["frame"],
        bevel_width=0.0022,
        bevel_segments=2,
    )

    # Dark horizontal shadow groove right above the rail ridge (y=113.5) and lower rail step (y=102.0)
    groove_y, groove_z = px(268.0, 113.5)
    step_y, step_z = px(268.0, 101.5)
    for side in (-1.0, 1.0):
        bpy.ops.mesh.primitive_cube_add(size=1.0, location=(side * 0.0142, groove_y, groove_z))
        g_cut = bpy.context.active_object
        g_cut.scale = (0.0026, px_len(150.0), px_len(2.6))
        bpy.ops.object.transform_apply(scale=True)
        boolean_op(body, g_cut, "DIFFERENCE")

        bpy.ops.mesh.primitive_cube_add(size=1.0, location=(side * 0.0154, step_y, step_z))
        s_cut = bpy.context.active_object
        s_cut.scale = (0.0022, px_len(150.0), px_len(3.8))
        bpy.ops.object.transform_apply(scale=True)
        boolean_op(rail, s_cut, "DIFFERENCE")

    # Two angled underside notches near the front of the rail: x=320 and x=331, y=97.2
    for px_slot in (320.0, 331.0):
        slot_y, slot_z = px(px_slot, 97.5)
        bpy.ops.mesh.primitive_cube_add(
            size=1.0,
            location=(0.0, slot_y, slot_z),
            rotation=(math.radians(22.0), 0.0, 0.0),
        )
        slot = bpy.context.active_object
        slot.scale = (0.038, px_len(4.5), px_len(7.5))
        bpy.ops.object.transform_apply(rotation=True, scale=True)
        boolean_op(rail, slot, "DIFFERENCE")
        bpy.ops.mesh.primitive_cube_add(
            size=1.0,
            location=(0.0, slot_y, slot_z),
            rotation=(math.radians(22.0), 0.0, 0.0),
        )
        slot2 = bpy.context.active_object
        slot2.scale = (0.038, px_len(4.5), px_len(7.5))
        bpy.ops.object.transform_apply(rotation=True, scale=True)
        boolean_op(body, slot2, "DIFFERENCE")

    # Subtle circular emblem on forward dust cover: x=306, y=124, r=12 px
    emb_y, emb_z = px(306.0, 124.0)
    for side in (-1.0, 1.0):
        bpy.ops.mesh.primitive_cylinder_add(
            vertices=32,
            radius=px_len(11.5),
            depth=0.0008,
            location=(side * 0.0140, emb_y, emb_z),
            rotation=(0.0, math.radians(90.0), 0.0),
        )
        medallion = bpy.context.active_object
        bpy.ops.object.transform_apply(rotation=True)
        boolean_op(body, medallion, "DIFFERENCE")

    bpy.ops.object.select_all(action="DESELECT")
    body.select_set(True)
    rail.select_set(True)
    bpy.context.view_layer.objects.active = body
    bpy.ops.object.join()
    body.name = "frame"
    add_edge_split_sharp(body, 34.0)
    return body


def build_grip_panels(mats: dict[str, bpy.types.Material]) -> bpy.types.Object:
    """Build the wide tapered diamond-checkered grip panel with a crisp ~4.2 px frame border and no safety notch."""
    # Large full-coverage panel (~41.5-42 px wide) inset by a uniform ~4.2-4.5 px from the
    # backstrap, frontstrap, and baseplate edges so there is a clean, even border gap all around.
    panel_px = [
        (74.5, 123.8),   # Top-rear corner (~4.5 px inset from upper backstrap/tang)
        (116.5, 123.8),  # Top-front corner (below trigger guard root)
        (113.0, 110.0),  # Upper-front edge
        (109.5, 94.0),   # Upper-mid front edge (smooth continuous line, no safety notch)
        (106.0, 80.5),   # Mid front edge (~4.5 px inset from frontstrap)
        (101.5, 69.5),   # Lower-mid front edge (~4.5 px inset from frontstrap)
        (97.2, 57.5),    # Lower front edge (~4.5 px inset from frontstrap)
        (94.0, 45.5),    # Near-bottom front edge (~4.5 px inset from frontstrap)
        (91.5, 35.5),    # Bottom-front corner (~4.5 px inset from frontstrap & baseplate)
        (50.0, 35.5),    # Bottom-rear corner (~4.2 px inset from backstrap & baseplate)
        (50.8, 46.0),    # Lower rear edge (~4.2 px inset from backstrap)
        (54.2, 58.0),    # Lower-mid rear edge (~4.2 px inset from backstrap)
        (59.0, 70.0),    # Mid rear edge (~4.2 px inset from backstrap)
        (64.0, 82.0),    # Upper-mid rear edge (~4.2 px inset from backstrap)
        (69.0, 94.0),    # Upper rear edge (~4.2 px inset from backstrap)
        (73.0, 104.0),   # Thumb web lower curve (~4.2 px inset from backstrap)
        (75.0, 114.0),   # Thumb web upper curve (~4.5 px inset from backstrap)
    ]
    panel_profile = [px(x, y) for (x, y) in panel_px]

    bm = bmesh.new()
    w_inner = 0.0130
    w_outer = 0.0164
    loop_2d = [Vector((y, z, 0.0)) for (y, z) in panel_profile]
    tris = tessellate_polygon([loop_2d])

    for side in (-1.0, 1.0):
        x0 = side * w_inner
        x1 = side * w_outer
        v0 = [bm.verts.new((x0, y, z)) for (y, z) in panel_profile]
        v1 = [bm.verts.new((x1, y, z)) for (y, z) in panel_profile]
        for i0, i1, i2 in tris:
            if side > 0:
                bm.faces.new((v0[i0], v0[i2], v0[i1]))
                bm.faces.new((v1[i0], v1[i1], v1[i2]))
            else:
                bm.faces.new((v0[i0], v0[i1], v0[i2]))
                bm.faces.new((v1[i0], v1[i2], v1[i1]))
        n = len(panel_profile)
        for i in range(n):
            j = (i + 1) % n
            if side > 0:
                bm.faces.new((v0[i], v0[j], v1[j], v1[i]))
            else:
                bm.faces.new((v1[i], v1[j], v0[j], v0[i]))

    panels = create_mesh_object("grip_panels", bm, mats["grip"])

    bev = panels.modifiers.new(name="Bevel", type="BEVEL")
    bev.width = 0.0012
    bev.segments = 3
    bev.limit_method = "ANGLE"
    bev.angle_limit = math.radians(28.0)
    bpy.context.view_layer.objects.active = panels
    bpy.ops.object.modifier_apply(modifier=bev.name)

    # Two recessed circular screw counterbores centered along the wide grip panel
    for px_sx, px_sy in [(83.5, 78.0), (74.2, 53.0)]:
        y_s, z_s = px(px_sx, px_sy)
        for side in (-1.0, 1.0):
            bpy.ops.mesh.primitive_cylinder_add(
                vertices=24,
                radius=px_len(5.0),
                depth=0.0038,
                location=(side * 0.0160, y_s, z_s),
                rotation=(0.0, math.radians(90.0), 0.0),
            )
            hole = bpy.context.active_object
            bpy.ops.object.transform_apply(rotation=True)
            boolean_op(panels, hole, "DIFFERENCE")

    add_edge_split_sharp(panels, 34.0)
    return panels


def build_trigger(mats: dict[str, bpy.types.Material]) -> bpy.types.Object:
    """Build the crescent trigger traced from exact pixel coordinates (x=132..147, y=90..118.5)."""
    trig_px = [
        (139.0, 119.5),
        (147.5, 119.5),
        (143.5, 112.0),
        (139.0, 106.0),
        (135.8, 100.0),
        (136.5, 94.0),
        (139.0, 90.0),
        (135.5, 90.0),
        (132.5, 94.5),
        (132.2, 101.0),
        (134.8, 108.5),
    ]
    trigger = extrude_2d_profile_x(
        "trigger",
        [px(x, y) for (x, y) in trig_px],
        half_width=0.0030,
        mat=mats["steel"],
        bevel_width=0.0005,
        bevel_segments=2,
    )
    add_edge_split_sharp(trigger, 34.0)
    return trigger


def build_controls_and_pins(mats: dict[str, bpy.types.Material]) -> bpy.types.Object:
    """Build the takedown button, frame pins, thumb mag catch, grip screws, and rear lanyard loop."""
    parts: list[bpy.types.Object] = []

    # 1. Circular takedown button: center (193.0, 132.0), radius=7.5 px
    td_y, td_z = px(193.0, 132.0)
    bpy.ops.mesh.primitive_cylinder_add(
        vertices=32,
        radius=px_len(7.5),
        depth=0.0300,
        location=(0.0, td_y, td_z),
        rotation=(0.0, math.radians(90.0), 0.0),
    )
    takedown = bpy.context.active_object
    bpy.ops.object.transform_apply(rotation=True)
    bev = takedown.modifiers.new(name="Bevel", type="BEVEL")
    bev.width = 0.0008
    bev.segments = 2
    bpy.ops.object.modifier_apply(modifier=bev.name)
    takedown.data.materials.append(mats["steel"])
    parts.append(takedown)

    # 2. Small trigger pin at (141.5, 130.0) and rear frame pin at (67.0, 132.0)
    for px_px, px_py in [(141.5, 130.0), (67.0, 132.0)]:
        p_y, p_z = px(px_px, px_py)
        bpy.ops.mesh.primitive_cylinder_add(
            vertices=20,
            radius=px_len(2.0),
            depth=0.0298,
            location=(0.0, p_y, p_z),
            rotation=(0.0, math.radians(90.0), 0.0),
        )
        pin = bpy.context.active_object
        bpy.ops.object.transform_apply(rotation=True)
        pin.data.materials.append(mats["steel"])
        parts.append(pin)

    # 3. Grip panel screws inside the counterbores at (83.5, 78.0) and (74.2, 53.0)
    for px_sx, px_sy in [(83.5, 78.0), (74.2, 53.0)]:
        y_s, z_s = px(px_sx, px_sy)
        bpy.ops.mesh.primitive_cylinder_add(
            vertices=20,
            radius=px_len(2.6),
            depth=0.0306,
            location=(0.0, y_s, z_s),
            rotation=(0.0, math.radians(90.0), 0.0),
        )
        scr = bpy.context.active_object
        bpy.ops.object.transform_apply(rotation=True)
        scr.data.materials.append(mats["steel"])
        parts.append(scr)

    # 5. Rear figure-8 lanyard ring / hammer loop at x=40..55, y=133..147
    loop_px = [
        (55.5, 134.0),
        (43.0, 134.0),
        (40.0, 137.5),
        (42.5, 143.0),
        (48.0, 147.0),
        (52.5, 146.5),
        (57.5, 142.0),
    ]
    loop = extrude_2d_profile_x(
        "lanyard_loop",
        [px(x, y) for (x, y) in loop_px],
        half_width=0.0024,
        mat=mats["steel"],
        bevel_width=0.0006,
        bevel_segments=2,
    )
    for hx, hy, hr in [(45.5, 138.2, 2.5), (50.0, 142.5, 2.2)]:
        h_y, h_z = px(hx, hy)
        bpy.ops.mesh.primitive_cylinder_add(
            vertices=20,
            radius=px_len(hr),
            depth=0.012,
            location=(0.0, h_y, h_z),
            rotation=(0.0, math.radians(90.0), 0.0),
        )
        hole = bpy.context.active_object
        bpy.ops.object.transform_apply(rotation=True)
        boolean_op(loop, hole, "DIFFERENCE")
    parts.append(loop)

    bpy.ops.object.select_all(action="DESELECT")
    for p in parts:
        p.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.join()
    ctrl = bpy.context.active_object
    ctrl.name = "controls"
    add_edge_split_sharp(ctrl, 34.0)
    return ctrl


def build_magazine(mats: dict[str, bpy.types.Material]) -> bpy.types.Object:
    """Build the removable magazine body and compact baseplate (x=47..95, y=26.5..32.5)."""
    mag_body_px = [
        (90.0, 32.5),
        (51.0, 33.0),
        (72.0, 95.0),
        (105.0, 95.0),
    ]
    body = extrude_2d_profile_x(
        "mag_body",
        [px(x, y) for (x, y) in mag_body_px],
        half_width=0.0090,
        mat=mats["steel"],
        bevel_width=0.0010,
    )

    rim_px = [
        (94.8, 32.0),
        (47.2, 32.6),
        (47.8, 29.5),
        (94.5, 28.8),
    ]
    rim = extrude_2d_profile_x(
        "mag_rim",
        [px(x, y) for (x, y) in rim_px],
        half_width=0.0124,
        mat=mats["chrome"],
        bevel_width=0.0005,
    )

    pad_px = [
        (93.8, 28.8),
        (49.0, 29.5),
        (50.5, 26.8),
        (93.0, 26.2),
    ]
    pad = extrude_2d_profile_x(
        "mag_pad",
        [px(x, y) for (x, y) in pad_px],
        half_width=0.0116,
        mat=mats["steel"],
        bevel_width=0.0007,
    )

    bpy.ops.object.select_all(action="DESELECT")
    body.select_set(True)
    rim.select_set(True)
    pad.select_set(True)
    bpy.context.view_layer.objects.active = body
    bpy.ops.object.join()
    body.name = "magazine"
    add_edge_split_sharp(body, 34.0)
    return body


def add_weapon_sockets(parent: bpy.types.Object | None = None) -> list[bpy.types.Object]:
    """Create standard weapon attachment empties per docs/CONTENT.md §4.2 and weapons_batch1 conventions."""
    y_grip, z_grip = px(85.0, 80.0)
    y_muz, z_muz = px(360.0, 152.5)
    y_ej, z_ej = px(149.0, 159.0)
    y_si, z_si = px(75.0, 171.5)
    y_mag, z_mag = px(71.0, 28.0)

    socket_specs = [
        ("socket_grip", (0.0, y_grip, z_grip)),
        ("socket_muzzle", (0.0, y_muz, z_muz)),
        ("socket_eject", (0.014, y_ej, z_ej)),
        ("socket_sight", (0.0, y_si, z_si)),
        ("socket_mag", (0.0, y_mag, z_mag)),
        ("PISTOL_Socket_Muzzle", (0.0, y_muz, z_muz)),
        ("PISTOL_Socket_Eject", (0.014, y_ej, z_ej)),
        ("PISTOL_Socket_Sight", (0.0, y_si, z_si)),
        ("PISTOL_Socket_MagWell", (0.0, y_mag, z_mag)),
    ]
    sockets: list[bpy.types.Object] = []
    for name, loc in socket_specs:
        empty = bpy.data.objects.new(name, None)
        empty.empty_display_type = "ARROWS"
        empty.empty_display_size = 0.018
        empty.location = Vector(loc)
        if parent is not None:
            empty.parent = parent
        bpy.context.collection.objects.link(empty)
        sockets.append(empty)
    return sockets


def set_origin(obj: bpy.types.Object, pivot: tuple[float, float, float]) -> None:
    """Move object origin to world point `pivot` without moving its geometry."""
    p = Vector(pivot)
    obj.data.transform(Matrix.Translation(-p))
    obj.location = obj.location + p


def setup_hierarchy_and_clips(
    mesh_objects: list[bpy.types.Object],
    slide: bpy.types.Object,
    trigger: bpy.types.Object,
    magazine: bpy.types.Object,
) -> tuple[bpy.types.Object, list[dict[str, object]]]:
    """Build the PISTOL -> PISTOL_Pivot hierarchy and 30 fps TRS animation clips."""
    root = bpy.data.objects.new("PISTOL", None)
    root.empty_display_size = 0.03
    bpy.context.collection.objects.link(root)

    pivot = bpy.data.objects.new("PISTOL_Pivot", None)
    pivot.empty_display_size = 0.025
    pivot.parent = root
    bpy.context.collection.objects.link(pivot)

    set_origin(slide, (0.0, *px(200.0, 154.0)))
    set_origin(trigger, (0.0, *px(141.5, 118.5)))
    set_origin(magazine, (0.0, *px(71.0, 32.0)))

    for obj in mesh_objects:
        obj.parent = pivot

    add_weapon_sockets(parent=pivot)

    anim_objs = [pivot, slide, trigger, magazine]
    rest: dict[str, tuple[Vector, Euler, Vector]] = {}
    for obj in anim_objs:
        obj.rotation_mode = "XYZ"
        rest[obj.name] = (obj.location.copy(), obj.rotation_euler.copy(), obj.scale.copy())

    clips_meta: list[dict[str, object]] = []

    def add_clip(
        name: str,
        length: int,
        keys: dict[bpy.types.Object, list[tuple[int, dict[str, tuple[float, float, float]]]]],
        loop: bool = False,
        desc: str = "",
    ) -> None:
        clips_meta.append({"name": name, "frames": length, "loop": loop, "desc": desc})
        km = {k.name: v for k, v in keys.items()}
        for ob in anim_objs:
            l0, r0, s0 = rest[ob.name]
            ks = sorted(km.get(ob.name, []), key=lambda x: x[0])
            frames = [f for f, _ in ks]
            if 0 not in frames:
                ks.insert(0, (0, {}))
            if length not in frames:
                ks.append((length, ks[0][1] if loop else {}))
            ad = ob.animation_data_create()
            act = bpy.data.actions.new(f"{name}__{ob.name}")
            ad.action = act
            for f, d in ks:
                ob.location = l0 + Vector(d.get("l", (0.0, 0.0, 0.0)))
                r = d.get("r", (0.0, 0.0, 0.0))
                ob.rotation_euler = Euler(
                    (
                        r0.x + math.radians(r[0]),
                        r0.y + math.radians(r[1]),
                        r0.z + math.radians(r[2]),
                    )
                )
                s = d.get("s", (1.0, 1.0, 1.0))
                ob.scale = Vector((s0.x * s[0], s0.y * s[1], s0.z * s[2]))
                for prop in ("location", "rotation_euler", "scale"):
                    ob.keyframe_insert(prop, frame=f)
            ad.action = None
            tr = ad.nla_tracks.new()
            tr.name = name
            st = tr.strips.new(name, 0, act)
            st.extrapolation = "NOTHING"
            ob.location, ob.rotation_euler, ob.scale = l0.copy(), r0.copy(), s0.copy()

    add_clip(
        "Idle",
        60,
        {
            pivot: [(30, {"l": (0.0, 0.0, -0.002), "r": (0.4, 0.25, 0.3)})],
        },
        loop=True,
        desc="breathing sway",
    )
    add_clip(
        "Fire",
        9,
        {
            pivot: [
                (1, {"l": (0.0, 0.018, 0.006), "r": (-6.5, 0.6, 0.8)}),
                (4, {"l": (0.0, 0.006, 0.002), "r": (-2.0, 0.2, 0.2)}),
            ],
            slide: [(1, {"l": (0.0, 0.032, 0.0)}), (3, {"l": (0.0, 0.036, 0.0)}), (6, {})],
            trigger: [(1, {"r": (16.0, 0.0, 0.0)}), (4, {"r": (16.0, 0.0, 0.0)}), (8, {})],
        },
        loop=False,
        desc="one shot: muzzle flip, slide blowback, trigger pull",
    )
    add_clip(
        "Reload",
        54,
        {
            pivot: [
                (8, {"l": (0.0, 0.0, 0.018), "r": (6.0, 22.0, -10.0)}),
                (40, {"l": (0.0, 0.0, 0.018), "r": (7.0, 24.0, -10.0)}),
                (48, {"r": (1.5, 3.0, -1.5)}),
            ],
            magazine: [
                (8, {}),
                (18, {"l": (0.0, 0.035, -0.14)}),
                (26, {"l": (0.0, 0.040, -0.16)}),
                (36, {"l": (0.0, 0.0, 0.002)}),
                (38, {}),
            ],
            slide: [
                (40, {}),
                (43, {"l": (0.0, 0.034, 0.0)}),
                (46, {}),
            ],
        },
        loop=False,
        desc="drop magazine along grip angle, insert fresh magazine, rack slide",
    )
    add_clip(
        "Equip",
        24,
        {
            pivot: [
                (0, {"l": (0.0, 0.04, -0.15), "r": (45.0, 0.0, 20.0)}),
                (14, {"l": (0.0, -0.003, 0.003), "r": (-2.0, 0.0, -1.0)}),
                (24, {}),
            ],
            slide: [(12, {}), (16, {"l": (0.0, 0.032, 0.0)}), (20, {})],
        },
        loop=False,
        desc="draw from holster and rack slide",
    )
    add_clip(
        "Inspect",
        90,
        {
            pivot: [
                (18, {"l": (0.0, -0.015, 0.025), "r": (8.0, 34.0, -40.0)}),
                (40, {"l": (0.0, -0.015, 0.025), "r": (5.0, 28.0, -34.0)}),
                (58, {"l": (0.0, -0.015, 0.025), "r": (-6.0, -32.0, 42.0)}),
                (76, {"l": (0.0, -0.015, 0.025), "r": (-4.0, -26.0, 36.0)}),
                (90, {}),
            ],
            slide: [(58, {}), (64, {"l": (0.0, 0.024, 0.0)}), (72, {"l": (0.0, 0.024, 0.0)}), (77, {})],
        },
        loop=False,
        desc="showcase left and right profiles with press-check",
    )

    return root, clips_meta


def inspect_glb(path: Path) -> dict[str, object]:
    """Inspect a GLB file and return the weapons_batch1 report dictionary."""
    data = path.read_bytes()
    jl = struct.unpack("<I", data[12:16])[0]
    j = json.loads(data[20 : 20 + jl])
    acc = j.get("accessors", [])
    tris = 0
    prims = 0
    for m in j.get("meshes", []):
        for p in m["primitives"]:
            prims += 1
            if "indices" in p:
                tris += acc[p["indices"]]["count"] // 3
            else:
                tris += acc[p["attributes"]["POSITION"]]["count"] // 3
    anims = []
    for a in j.get("animations", []):
        dur = max(acc[s["input"]]["max"][0] for s in a["samplers"])
        paths = sorted({c["target"]["path"] for c in a["channels"]})
        nodes = sorted(
            {j["nodes"][c["target"]["node"]]["name"] for c in a["channels"] if "node" in c["target"]}
        )
        interp = sorted({s.get("interpolation", "LINEAR") for s in a["samplers"]})
        anims.append(
            {
                "name": a["name"],
                "seconds": round(dur, 4),
                "frames": int(round(dur * 30)),
                "channels": len(a["channels"]),
                "paths": paths,
                "nodes": nodes,
                "interp": interp,
            }
        )
    return {
        "file": path.name,
        "kb": len(data) // 1024,
        "tris": tris,
        "primitives": prims,
        "meshes": len(j.get("meshes", [])),
        "materials": [m["name"] for m in j.get("materials", [])],
        "images": [],
        "animations": anims,
        "extensionsUsed": j.get("extensionsUsed", []),
        "extensionsRequired": j.get("extensionsRequired", []),
        "scenes": len(j.get("scenes", [])),
        "nodes": [n.get("name") for n in j.get("nodes", [])],
        "skins": len(j.get("skins", [])),
    }


def node_tree_lines(path: Path) -> list[str]:
    data = path.read_bytes()
    jl = struct.unpack("<I", data[12:16])[0]
    j = json.loads(data[20 : 20 + jl])
    lines: list[str] = []

    def walk(idx: int, depth: int) -> None:
        n = j["nodes"][idx]
        kind = "mesh" if "mesh" in n else "node"
        lines.append("  " * depth + f"- {n.get('name')} ({kind})")
        for c in n.get("children", []):
            walk(c, depth + 1)

    for r in j["scenes"][j.get("scene", 0)]["nodes"]:
        walk(r, 0)
    return lines


def apply_transforms(objects: list[bpy.types.Object]) -> None:
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects:
        if obj.type == "MESH":
            obj.select_set(True)
            obj.data.name = obj.name
    bpy.context.view_layer.objects.active = objects[0]
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)


def setup_studio_lighting() -> list[bpy.types.Object]:
    """Create balanced studio lights for preview renders (removed before saving/exporting)."""
    temp_objs: list[bpy.types.Object] = []

    top_data = bpy.data.lights.new(name="TopStrip", type="AREA")
    top_data.energy = 3.6
    top_data.color = (0.95, 0.97, 1.0)
    top_data.shape = "RECTANGLE"
    top_data.size = 0.38
    top_data.size_y = 0.06
    top_obj = bpy.data.objects.new(name="TopStrip", object_data=top_data)
    top_obj.location = (-0.12, -0.08, 0.28)
    direction = Vector((0.0, -0.08, 0.05)) - top_obj.location
    top_obj.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
    bpy.context.collection.objects.link(top_obj)
    temp_objs.append(top_obj)

    left_data = bpy.data.lights.new(name="LeftKey", type="AREA")
    left_data.energy = 2.6
    left_data.color = (0.96, 0.98, 1.0)
    left_data.size = 0.52
    left_obj = bpy.data.objects.new(name="LeftKey", object_data=left_data)
    left_obj.location = (-0.48, -0.09, 0.08)
    direction = Vector((0.0, -0.08, 0.0)) - left_obj.location
    left_obj.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
    bpy.context.collection.objects.link(left_obj)
    temp_objs.append(left_obj)

    rim_data = bpy.data.lights.new(name="FrontRim", type="AREA")
    rim_data.energy = 2.2
    rim_data.color = (1.0, 0.95, 0.88)
    rim_data.size = 0.24
    rim_obj = bpy.data.objects.new(name="FrontRim", object_data=rim_data)
    rim_obj.location = (-0.18, -0.36, 0.05)
    direction = Vector((0.0, -0.18, 0.03)) - rim_obj.location
    rim_obj.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
    bpy.context.collection.objects.link(rim_obj)
    temp_objs.append(rim_obj)

    return temp_objs


def render_previews(render_side: Path | None, render_persp: Path | None) -> None:
    if render_side is None and render_persp is None:
        return

    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.samples = 64
    scene.cycles.use_denoising = False
    scene.render.film_transparent = True

    world = bpy.data.worlds.new("StudioWorld")
    bg = world.node_tree.nodes.get("Background")
    if bg:
        bg.inputs["Color"].default_value = (0.06, 0.065, 0.08, 1.0)
        bg.inputs["Strength"].default_value = 0.38
    scene.world = world

    temp_objs = setup_studio_lighting()

    cam_data = bpy.data.cameras.new(name="PreviewCam")
    cam_obj = bpy.data.objects.new(name="PreviewCam", object_data=cam_data)
    bpy.context.collection.objects.link(cam_obj)
    scene.camera = cam_obj
    temp_objs.append(cam_obj)

    if render_side is not None:
        render_side.parent.mkdir(parents=True, exist_ok=True)
        # Exact 1:1 orthographic framing matching the 400x200 reference image center (x=200, y=100) and width (400 px)!
        cam_data.type = "ORTHO"
        cam_data.ortho_scale = 400.0 * PX_SCALE
        cy, cz = px(200.0, 100.0)
        target = Vector((0.0, cy, cz))
        cam_obj.location = Vector((-0.55, cy, cz))
        direction = target - cam_obj.location
        cam_obj.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
        scene.render.resolution_x = 1200
        scene.render.resolution_y = 600
        scene.render.filepath = str(render_side.resolve())
        bpy.ops.render.render(write_still=True)

    if render_persp is not None:
        render_persp.parent.mkdir(parents=True, exist_ok=True)
        cam_data.type = "PERSP"
        cam_data.lens = 68.0
        cy, cz = px(195.0, 98.0)
        target = Vector((0.0, cy, cz))
        cam_obj.location = Vector((-0.38, cy - 0.22, cz + 0.10))
        direction = target - cam_obj.location
        cam_obj.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
        scene.render.resolution_x = 1200
        scene.render.resolution_y = 720
        scene.render.filepath = str(render_persp.resolve())
        bpy.ops.render.render(write_still=True)

    for obj in temp_objs:
        bpy.data.objects.remove(obj, do_unlink=True)


def main() -> None:
    args = parse_args()
    reset_scene()
    mats = make_materials(args.color)

    slide = build_slide(mats)
    barrel = build_barrel_and_muzzle(mats)
    sights = build_sights(mats)
    frame = build_frame(mats)
    grip_panels = build_grip_panels(mats)
    trigger = build_trigger(mats)
    controls = build_controls_and_pins(mats)
    magazine = build_magazine(mats)

    mesh_objects = [slide, barrel, sights, frame, grip_panels, trigger, controls, magazine]
    apply_transforms(mesh_objects)
    _, clips_meta = setup_hierarchy_and_clips(mesh_objects, slide, trigger, magazine)

    for obj in mesh_objects:
        tris = sum(len(p.vertices) - 2 for p in obj.data.polygons)
        if tris == 0:
            raise RuntimeError(f"Mesh object '{obj.name}' has 0 triangles!")
        print(f"  - {obj.name}: {tris} tris")

    total_tris = sum(
        sum(len(p.vertices) - 2 for p in obj.data.polygons)
        for obj in mesh_objects
    )
    print(f"[make_pistol] Built {len(mesh_objects)} mesh parts ({args.color}), {total_tris} triangles total.")

    render_previews(args.render, args.render_persp)

    args.blend.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(args.blend.resolve()))
    print(f"[make_pistol] Saved .blend -> {args.blend}")

    args.glb.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.export_scene.gltf(
        filepath=str(args.glb.resolve()),
        export_format="GLB",
        export_yup=True,
        export_apply=True,
        export_texcoords=True,
        export_normals=True,
        export_tangents=False,
        export_materials="EXPORT",
        export_cameras=False,
        export_lights=False,
        export_extras=False,
        export_animations=True,
        export_animation_mode="NLA_TRACKS",
        export_force_sampling=True,
        export_frame_step=1,
        export_optimize_animation_size=False,
        export_anim_slide_to_zero=True,
        export_pointer_animation=False,
        export_morph=False,
        export_skins=False,
        export_draco_mesh_compression_enable=False,
    )
    print(f"[make_pistol] Exported .glb -> {args.glb}")

    if args.batch_root is not None:
        batch_root = args.batch_root.resolve()
        bdir = batch_root / "blender" / "pistol"
        edir = batch_root / "export"
        rdir = batch_root / "renders"
        for d in (bdir, edir, rdir):
            d.mkdir(parents=True, exist_ok=True)

        stem = "pistol" if args.color == "midnight" else f"pistol_{args.color}"
        copy_if_different(args.blend, bdir / f"{stem}.blend")
        glb_dest = edir / f"{stem}.glb"
        copy_if_different(args.glb, glb_dest)

        if args.color == "midnight":
            glbm_dest = edir / "pistol_mobile.glb"
            copy_if_different(args.glb, glbm_dest)
            rep = {
                "desktop": inspect_glb(glb_dest),
                "mobile": inspect_glb(glbm_dest),
                "clips": clips_meta,
                "hierarchy": node_tree_lines(glb_dest),
                "textures_2k": {},
                "textures_1k": {},
            }
            rep_path = edir / "pistol_report.json"
            rep_path.write_text(json.dumps(rep, indent=1), encoding="utf-8")
            print(f"[make_pistol] Wrote batch report -> {rep_path}")

        if args.render is not None and args.render.exists():
            copy_if_different(args.render, rdir / f"pistol_side_{args.color}.png")
            if args.color == "midnight":
                copy_if_different(args.render, rdir / "pistol_hero.png")
            else:
                copy_if_different(args.render, rdir / f"pistol_{args.color}_hero.png")
        if args.render_persp is not None and args.render_persp.exists():
            copy_if_different(args.render_persp, rdir / f"pistol_persp_{args.color}.png")


if __name__ == "__main__":
    main()
