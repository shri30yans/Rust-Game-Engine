"""Generate the Blender modular-kit arena, export its GLB and cook both game copies."""

from __future__ import annotations

import argparse
import importlib.util
import hashlib
import subprocess
import sys
from pathlib import Path

import bpy
from mathutils import Matrix, Vector


ROOT = Path(__file__).resolve().parents[2]
BLENDER_EXPORT = ROOT / "tools/blender/export.py"


def load_exporter():
    """Load the canonical exporter so generation and manual exports share validation."""
    spec = importlib.util.spec_from_file_location("world_pipeline_export", BLENDER_EXPORT)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load exporter at {BLENDER_EXPORT}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


EXPORT = load_exporter()


def parse_args() -> argparse.Namespace:
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    parser = argparse.ArgumentParser(description="Generate and cook the sample modular arena.")
    parser.add_argument(
        "--blend",
        type=Path,
        default=ROOT / "games/sandbox/content/blender/sample_arena.blend",
    )
    parser.add_argument(
        "--source",
        type=Path,
        default=ROOT / "games/sandbox/content/source",
    )
    parser.add_argument(
        "--sandbox-content",
        type=Path,
        default=ROOT / "games/sandbox/content",
    )
    parser.add_argument(
        "--imagine-content",
        type=Path,
        default=ROOT / "games/imagine/content",
    )
    parser.add_argument("--skip-cook", action="store_true")
    return parser.parse_args(argv)


def next_id(name: str) -> str:
    """Return a stable non-zero ID derived from the authored object name."""
    value = int.from_bytes(hashlib.sha256(f"sample-arena:{name}".encode()).digest()[:8], "big")
    return f"0x{value or 1:016x}"


def set_id(obj: bpy.types.ID) -> int:
    value = next_id(obj.name)
    obj["asset_id"] = value
    return int(value, 16)


def cube_vertices(size: tuple[float, float, float]) -> list[tuple[float, float, float]]:
    x, y, z = size
    return [
        (0, 0, 0), (x, 0, 0), (x, y, 0), (0, y, 0),
        (0, 0, z), (x, 0, z), (x, y, z), (0, y, z),
    ]


CUBE_FACES = [
    (0, 3, 2, 1), (4, 5, 6, 7), (0, 1, 5, 4),
    (1, 2, 6, 5), (2, 3, 7, 6), (3, 0, 4, 7),
]


def material_with_trim_sheet() -> bpy.types.Material:
    material = bpy.data.materials.new("arena_trim_sheet")
    material.use_nodes = True
    nodes = material.node_tree.nodes
    links = material.node_tree.links
    shader = nodes.get("Principled BSDF")
    if shader is None:
        raise RuntimeError("Blender Principled BSDF node is missing")
    shader.inputs["Metallic"].default_value = 0.3
    shader.inputs["Roughness"].default_value = 0.72

    base = bpy.data.images.new("arena_trim_base_color", width=16, height=16, alpha=True)
    normal = bpy.data.images.new("arena_trim_normal", width=16, height=16, alpha=True)
    orm = bpy.data.images.new("arena_trim_orm", width=16, height=16, alpha=True)
    base_pixels = []
    normal_pixels = []
    orm_pixels = []
    for y in range(16):
        for x in range(16):
            seam = x in (0, 1, 14, 15) or y in (0, 1, 14, 15)
            base_pixels.extend((0.16, 0.21, 0.24, 1.0) if seam else (0.34, 0.38, 0.36, 1.0))
            normal_pixels.extend((0.5, 0.5, 1.0, 1.0))
            orm_pixels.extend((1.0, 0.68 if seam else 0.82, 0.12 if seam else 0.04, 1.0))
    for image, pixels in ((base, base_pixels), (normal, normal_pixels), (orm, orm_pixels)):
        image.pixels.foreach_set(pixels)
        image.pack()
    for image, label in ((base, "base_color"), (normal, "normal"), (orm, "orm")):
        node = nodes.new("ShaderNodeTexImage")
        node.name = f"trim_{label}"
        node.image = image
        if label == "base_color":
            links.new(node.outputs["Color"], shader.inputs["Base Color"])
        elif label == "normal":
            normal_map = nodes.new("ShaderNodeNormalMap")
            links.new(node.outputs["Color"], normal_map.inputs["Color"])
            links.new(normal_map.outputs["Normal"], shader.inputs["Normal"])
        else:
            separate = nodes.new("ShaderNodeSeparateColor")
            links.new(node.outputs["Color"], separate.inputs["Color"])
            links.new(separate.outputs["Green"], shader.inputs["Roughness"])
            links.new(separate.outputs["Blue"], shader.inputs["Metallic"])
    return material


def mesh_object(
    name: str,
    location: tuple[float, float, float],
    size: tuple[float, float, float],
    material: bpy.types.Material,
    role: str = "static",
    collision: str = "box",
    tag_values: dict[str, object] | None = None,
) -> bpy.types.Object:
    mesh = bpy.data.meshes.new(f"{name}_mesh")
    mesh.from_pydata(cube_vertices(size), [], CUBE_FACES)
    mesh.update()
    uv_layer = mesh.uv_layers.new(name="UVMap")
    for polygon in mesh.polygons:
        axis = max(range(3), key=lambda value: abs(polygon.normal[value]))
        axes = [value for value in range(3) if value != axis]
        for loop_index in polygon.loop_indices:
            coordinate = mesh.vertices[mesh.loops[loop_index].vertex_index].co
            uv_layer.data[loop_index].uv = (
                coordinate[axes[0]] / size[axes[0]],
                coordinate[axes[1]] / size[axes[1]],
            )
    mesh.materials.append(material)
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    obj.location = location
    obj["role"] = role
    obj["material"] = "steel"
    obj["collision"] = collision
    if tag_values:
        for key, value in tag_values.items():
            obj[key] = value
    set_id(obj)
    return obj


def parts_object(
    name: str,
    location: tuple[float, float, float],
    parts: list[tuple[tuple[float, float, float], tuple[float, float, float]]],
    bounds: tuple[float, float, float],
    material: bpy.types.Material,
) -> bpy.types.Object:
    vertices: list[tuple[float, float, float]] = []
    faces: list[tuple[int, ...]] = []
    for origin, size in parts:
        offset = len(vertices)
        vertices.extend(
            (x + origin[0], y + origin[1], z + origin[2])
            for x, y, z in cube_vertices(size)
        )
        faces.extend(tuple(index + offset for index in face) for face in CUBE_FACES)
    mesh = bpy.data.meshes.new(f"{name}_mesh")
    mesh.from_pydata(vertices, [], faces)
    mesh.update()
    uv_layer = mesh.uv_layers.new(name="UVMap")
    for polygon in mesh.polygons:
        axis = max(range(3), key=lambda value: abs(polygon.normal[value]))
        axes = [value for value in range(3) if value != axis]
        for loop_index in polygon.loop_indices:
            coordinate = mesh.vertices[mesh.loops[loop_index].vertex_index].co
            uv_layer.data[loop_index].uv = (
                coordinate[axes[0]] / bounds[axes[0]],
                coordinate[axes[1]] / bounds[axes[1]],
            )
    mesh.materials.append(material)
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    obj.location = location
    obj["role"] = "static"
    obj["material"] = "steel"
    obj["collision"] = "box"
    set_id(obj)
    return obj


def collision_child(parent: bpy.types.Object, size: tuple[float, float, float]) -> bpy.types.Object:
    child = mesh_object(
        f"{parent.name}_COL",
        (0.0, 0.0, 0.0),
        size,
        parent.data.materials[0],
        role="trigger" if parent.get("role") == "trigger" else "static",
        collision="box",
    )
    child.parent = parent
    child.matrix_parent_inverse = Matrix.Identity(4)
    child.location = (0.0, 0.0, 0.0)
    child["collision"] = "box"
    return child


def empty(name: str, location: tuple[float, float, float], tags: dict[str, object] | None = None) -> bpy.types.Object:
    obj = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(obj)
    obj.location = location
    if tags:
        for key, value in tags.items():
            obj[key] = value
    set_id(obj)
    return obj


def add_modular_kit(material: bpy.types.Material) -> None:
    kit_y = -8.0
    mesh_object("kit_wall_4m", (0.0, kit_y, 0.0), (4.0, 0.3, 4.0), material)
    parts_object(
        "kit_wall_with_door",
        (5.0, kit_y, 0.0),
        [
            ((0.0, 0.0, 0.0), (1.2, 0.3, 4.0)),
            ((2.8, 0.0, 0.0), (1.2, 0.3, 4.0)),
            ((1.2, 0.0, 3.0), (1.6, 0.3, 1.0)),
        ],
        (4.0, 0.3, 4.0),
        material,
    )
    mesh_object("kit_floor_4x4", (10.0, kit_y, 0.0), (4.0, 4.0, 0.2), material)
    parts_object(
        "kit_stairs",
        (15.0, kit_y, 0.0),
        [((0.0, step * 0.5, step * 0.4), (4.0, 0.5, (6 - step) * 0.4)) for step in range(6)],
        (4.0, 3.0, 2.4),
        material,
    )
    parts_object(
        "kit_ramp",
        (20.0, kit_y, 0.0),
        [((0.0, 0.0, 0.0), (4.0, 4.0, 1.4))],
        (4.0, 4.0, 1.4),
        material,
    )
    mesh_object("kit_pillar", (25.0, kit_y, 0.0), (0.8, 0.8, 4.0), material)
    mesh_object("kit_railing", (30.0, kit_y, 0.0), (4.0, 0.15, 1.1), material)


def add_arena(material: bpy.types.Material) -> list[int]:
    model_ids: dict[str, int] = {}
    for floor_z, width in ((0.0, 10), (5.0, 10)):
        for row in range(width):
            for column in range(width):
                tile = mesh_object(
                    f"floor_{int(floor_z)}_{row}_{column}",
                    (float(column * 4), float(row * 4), floor_z),
                    (4.0, 4.0, 0.2),
                    material,
                )
                model_ids.setdefault("floor", int(tile["asset_id"], 16))

    # Four-metre wall modules close the 40 m ground floor and upper walkway.
    for floor_z in (0.0, 5.0):
        for index in range(10):
            offset = float(index * 4)
            for side, x, y, size in (
                ("north", offset, 0.0, (4.0, 0.3, 4.0)),
                ("south", offset, 39.7, (4.0, 0.3, 4.0)),
                ("west", 0.0, offset, (0.3, 4.0, 4.0)),
                ("east", 39.7, offset, (0.3, 4.0, 4.0)),
            ):
                wall = mesh_object(
                    f"wall_{int(floor_z)}_{side}_{index}",
                    (x, y, floor_z),
                    size,
                    material,
                )
                collision_child(wall, size)
                model_ids.setdefault("wall", int(wall["asset_id"], 16))

    stairs = mesh_object("stairs_upper", (18.0, 16.0, 0.2), (4.0, 6.0, 2.4), material)
    collision_child(stairs, (4.0, 6.0, 2.4))
    ramp = mesh_object("ramp_upper", (26.0, 16.0, 0.2), (4.0, 6.0, 2.0), material)
    collision_child(ramp, (4.0, 6.0, 2.0))
    pillar = mesh_object("pillar_mid", (18.0, 20.0, 0.2), (1.0, 1.0, 4.8), material)
    collision_child(pillar, (1.0, 1.0, 4.8))
    railing = mesh_object("railing_upper", (12.0, 16.0, 5.2), (4.0, 0.15, 1.0), material)
    collision_child(railing, (4.0, 0.15, 1.0))

    door = mesh_object("door_a", (20.0, 20.0, 0.2), (4.0, 0.3, 3.8), material, role="mover")
    collision_child(door, (4.0, 0.3, 3.8))
    for layout, height in (("open", 4.0), ("lockdown", 0.2)):
        pose = empty(f"door_a@{layout}", (0.0, 0.0, 0.0))
        pose.parent = door
        pose.matrix_world = Matrix.Translation(Vector((20.0, 20.0, height)))

    switch = mesh_object("warning_light", (12.0, 20.0, 0.2), (0.5, 0.5, 0.6), material)
    switch["switch"] = "warning_light"
    switch["layout_open"] = "on"
    switch["layout_lockdown"] = "off"
    collision_child(switch, (0.5, 0.5, 0.6))

    for team, name, position in (
        (0, "spawn_red_1", (8.0, 8.0, 0.2)),
        (0, "spawn_red_2", (8.0, 32.0, 0.2)),
        (1, "spawn_blue_1", (32.0, 8.0, 0.2)),
        (1, "spawn_blue_2", (32.0, 32.0, 0.2)),
    ):
        empty(name, position, {"team": team})
    for team, prefix, positions in (
        (0, "player", ((8.0, 8.0, 0.2), (8.0, 32.0, 0.2))),
        (1, "bot", ((32.0, 8.0, 0.2), (32.0, 32.0, 0.2), (20.0, 8.0, 0.2), (20.0, 32.0, 0.2), (8.0, 20.0, 0.2), (32.0, 20.0, 0.2))),
    ):
        for index, position in enumerate(positions, start=1):
            empty(f"{prefix}_{index}", position, {"team": team})
    for name, position in (
        ("waypoint_west", (6.0, 6.0, 0.2)),
        ("waypoint_north", (10.0, 6.0, 0.2)),
        ("waypoint_east", (10.0, 10.0, 0.2)),
        ("waypoint_south", (6.0, 10.0, 0.2)),
    ):
        empty(name, position)

    empty("zone_red", (8.0, 20.0, 0.2), {"half_extents": "4,2,4", "team": 0})
    empty("zone_blue", (32.0, 20.0, 0.2), {"half_extents": "4,2,4", "team": 1})
    empty("obj_control", (20.0, 20.0, 0.2), {"half_extents": "2,1,2"})
    empty("probe_lower", (10.0, 10.0, 2.0), {"half_extents": "8,3,8"})
    empty("probe_upper", (30.0, 30.0, 7.0), {"half_extents": "8,3,8"})
    empty("sound_ambience", (20.0, 20.0, 2.0), {"half_extents": "20,8,20"})
    empty("CUE_lockdown", (20.0, 20.0, 0.2), {"layout": "lockdown", "duration_s": 1.25})

    for (kind, name, position, size) in (
        ("crate", "crate_a", (14.0, 14.0, 0.2), (1.0, 1.0, 1.0)),
        ("crate", "crate_b", (16.0, 14.0, 0.2), (1.0, 1.0, 1.0)),
        ("explosive_barrel", "barrel_a", (25.0, 22.0, 0.2), (0.8, 0.8, 1.4)),
        ("sliding_door", "door_prefab_a", (24.0, 20.0, 0.2), (2.0, 0.3, 3.0)),
        ("turret_base", "turret_a", (28.0, 14.0, 0.2), (1.0, 1.0, 1.4)),
    ):
        prop = mesh_object(name, position, size, material, role="prop")
        collision_child(prop, size)
        prop["prefab_id"] = 0
        model_ids.setdefault(kind, int(prop["asset_id"], 16))
        model_ids[f"placed:{name}"] = int(prop["asset_id"], 16)

    return [model_ids[key] for key in ("floor", "wall", "crate", "explosive_barrel", "sliding_door", "turret_base")]


def write_prefabs(source: Path) -> None:
    definitions = {
        "crate": ("crate_a", "Cooked", "wood", '{"health": {"hp": 100}}'),
        "explosive_barrel": ("barrel_a", "Cooked", "steel", '{"explosive": {"radius_m": 6.0}}'),
        "sliding_door": ("door_prefab_a", "Cooked", "steel", '{"door": {"open_s": 0.6}}'),
        "turret_base": ("turret_a", "Cooked", "steel", '{"health": {"hp": 200}}'),
    }
    prefabs = source / "prefabs"
    prefabs.mkdir(parents=True, exist_ok=True)
    expected = {f"{name}.prefab.ron" for name in definitions}
    for path in prefabs.glob("*.prefab.ron"):
        if path.name not in expected:
            path.unlink()
    for name, (object_name, collision, surface, components) in definitions.items():
        obj = bpy.data.objects.get(object_name)
        if obj is None:
            raise RuntimeError(f"missing prefab model object {object_name}")
        prefab_id = int(next_id(f"prefab:{name}"), 16)
        for placed in bpy.context.scene.objects:
            same_family = name == "crate" and placed.name in {"crate_a", "crate_b"}
            if placed.get("role") == "prop" and (placed.name == object_name or same_family):
                placed["prefab_id"] = f"0x{prefab_id:016x}"
        body = (
            f"(asset_id: {prefab_id}, model_id: {int(obj['asset_id'], 16)}, "
            f"collision: {collision}, surface: \"{surface}\", components: {components})"
        )
        (prefabs / f"{name}.prefab.ron").write_text(body + "\n", encoding="utf-8")


def reset_scene() -> None:
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.unit_settings.system = "METRIC"
    scene.unit_settings.scale_length = 1.0
    scene["asset_id"] = next_id("scene")
    scene["layouts"] = "open,lockdown"
    scene.render.fps = 60


def main() -> None:
    args = parse_args()
    reset_scene()
    material = material_with_trim_sheet()
    add_modular_kit(material)
    add_arena(material)
    args.source.mkdir(parents=True, exist_ok=True)
    args.blend.parent.mkdir(parents=True, exist_ok=True)
    write_prefabs(args.source)
    bpy.context.scene.render.filepath = ""
    bpy.ops.wm.save_as_mainfile(filepath=str(args.blend.resolve()))
    output = args.source / "sample_arena.glb"
    EXPORT.export_scene(output.resolve())
    if args.skip_cook:
        return
    for content in (args.sandbox_content, args.imagine_content):
        command = [
            "cargo",
            "run",
            "-p",
            "cook",
            "--",
            str(args.source.resolve()),
            str((content / "cooked").resolve()),
        ]
        subprocess.run(command, cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
