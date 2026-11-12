"""Validate a Blender scene and export its persistent metadata as glTF binary."""

from __future__ import annotations

import argparse
import math
import re
import secrets
import sys
from pathlib import Path

import bpy


KNOWN_ROLES = {"static", "mover", "breakable", "trigger", "interact", "prop", "glass"}
KNOWN_COLLISIONS = {"box", "hull", "mesh", "decompose"}
KNOWN_MATERIALS = {
    "concrete", "brick", "plaster", "wood", "steel", "metal_corrugated",
    "glass", "dirt", "gravel", "asphalt", "plastic_sheet", "rubber",
}
SOCKETS = {
    "socket_muzzle", "socket_eject", "socket_sight", "socket_grip",
    "socket_hand_r", "socket_back",
}
ID_MAX = (1 << 64) - 1
LAYOUT_NAME = re.compile(r"^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$")


def valid_layout_name(value: object) -> bool:
    """Whether a layout or switch name follows the authored lower_snake_case convention."""
    return isinstance(value, str) and LAYOUT_NAME.fullmatch(value) is not None


def scene_layouts(scene: bpy.types.Scene, errors: list[str]) -> list[str]:
    """Parse the ordered layout names and report malformed scene metadata once."""
    value = scene.get("layouts")
    if value is None:
        return []
    if not isinstance(value, str):
        errors.append("Scene: layouts must be an ordered comma-separated string")
        return []
    names = [name.strip() for name in value.split(",")]
    if not names or any(not valid_layout_name(name) for name in names):
        errors.append("Scene: layouts must be comma-separated lower_snake_case names")
        return []
    if len(names) != len(set(names)):
        errors.append("Scene: layouts must not contain duplicate names")
        return []
    return names


def validate_layout_objects(scene: bpy.types.Scene, layouts: list[str], errors: list[str]) -> None:
    """Check each authored mover pose, switch state and Director cue against scene layouts."""
    movers = [obj for obj in scene.objects if obj.get("role") == "mover"]
    for obj in movers:
        expected = {f"{obj.name}@{layout}" for layout in layouts}
        actual = {
            child.name
            for child in obj.children
            if child.name.startswith(f"{obj.name}@")
        }
        for name in sorted(expected - actual):
            errors.append(f"{obj.name}: add Empty pose child `{name}` for its layout")
        for name in sorted(actual - expected):
            errors.append(f"{obj.name}: remove extra layout pose child `{name}`")
        for child in obj.children:
            if child.name in expected and child.type != "EMPTY":
                errors.append(f"{obj.name}: layout pose `{child.name}` must be an Empty")

    for obj in scene.objects:
        switch = obj.get("switch")
        if switch is not None:
            if not valid_layout_name(switch):
                errors.append(f"{obj.name}: switch must be a lower_snake_case name")
            for layout in layouts:
                field = f"layout_{layout}"
                if field not in obj:
                    errors.append(f"{obj.name}: add custom property `{field}` with value on or off")
                elif obj[field] not in ("on", "off"):
                    errors.append(f"{obj.name}: `{field}` must be on or off")
        if not obj.name.startswith("CUE_"):
            continue
        cue_name = obj.name.removeprefix("CUE_")
        if obj.type != "EMPTY":
            errors.append(f"{obj.name}: Director cues must be Empty objects")
        if not valid_layout_name(cue_name):
            errors.append(f"{obj.name}: cue name after CUE_ must be lower_snake_case")
        target = obj.get("layout")
        if target not in layouts:
            errors.append(f"{obj.name}: layout must name one of the scene layouts")
        duration = obj.get("duration_s")
        if (
            not isinstance(duration, (int, float))
            or not math.isfinite(duration)
            or duration <= 0
        ):
            errors.append(f"{obj.name}: duration_s must be finite and greater than zero")


def asset_id(value: object) -> int:
    """Return a valid stored id, assigning one only when absent or malformed."""
    current = value.get("asset_id")
    try:
        parsed = int(current, 0) if isinstance(current, str) else current
    except ValueError:
        parsed = None
    if isinstance(parsed, int) and 0 < parsed <= ID_MAX:
        return parsed
    parsed = secrets.randbits(64) or 1
    value["asset_id"] = f"0x{parsed:016x}"
    return parsed


def validate_scene(scene: bpy.types.Scene) -> list[str]:
    """Assign persistent ids and report every authoring rule violation."""
    errors: list[str] = []
    used: set[int] = set()
    asset_id(scene)
    layouts = scene_layouts(scene, errors)
    if scene.unit_settings.system != "METRIC" or abs(scene.unit_settings.scale_length - 1.0) > 1e-5:
        errors.append("Scene: set units to Metric and Unit Scale to 1.0 (metres)")
    for obj in scene.objects:
        identifier = asset_id(obj)
        if identifier in used:
            identifier = secrets.randbits(64) or 1
            while identifier in used:
                identifier = secrets.randbits(64) or 1
            obj["asset_id"] = f"0x{identifier:016x}"
            print(f"WARNING: {obj.name}: duplicate asset_id re-assigned")
        used.add(identifier)
        label = obj.name
        if obj.type == "MESH":
            if any(abs(value - 1.0) > 1e-5 for value in obj.scale):
                errors.append(f"{label}: apply scale (Ctrl+A > Scale)")
            if any(abs(value) > 1e-5 for value in obj.rotation_euler):
                errors.append(f"{label}: apply rotation (Ctrl+A > Rotation)")
            bounds = [tuple(corner) for corner in obj.bound_box]
            if bounds and not any(
                abs(min(point[axis] for point in bounds)) <= 1e-5
                or abs(max(point[axis] for point in bounds)) <= 1e-5
                for axis in range(3)
            ):
                errors.append(f"{label}: move the origin to a ground or attachment point")
            if obj.name.endswith("_COL") and (
                obj.parent is None or obj.parent.name != obj.name[:-4]
            ):
                errors.append(
                    f"{label}: parent the _COL mesh to its matching render object `{obj.name[:-4]}`"
                )
            if "_LOD" in obj.name and not any(obj.name.endswith(f"_LOD{i}") for i in range(3)):
                errors.append(f"{label}: use the _LOD0, _LOD1 or _LOD2 suffix")
        role = obj.get("role")
        if role is not None and role not in KNOWN_ROLES:
            errors.append(f"{label}: role must be one of {', '.join(sorted(KNOWN_ROLES))}")
        collision = obj.get("collision")
        if collision is not None and collision not in KNOWN_COLLISIONS:
            errors.append(f"{label}: collision must be one of {', '.join(sorted(KNOWN_COLLISIONS))}")
        material = obj.get("material")
        if material is not None and material not in KNOWN_MATERIALS:
            errors.append(f"{label}: material must use a known surface tag")
        team = obj.get("team")
        if team is not None and (not isinstance(team, int) or not 0 <= team <= 3):
            errors.append(f"{label}: team must be an integer from 0 to 3")
        cue = obj.get("cue")
        if cue is not None and (not isinstance(cue, str) or not cue.strip()):
            errors.append(f"{label}: cue must be a non-empty Director event name")
        if obj.name.startswith("socket_") and obj.type != "EMPTY":
            errors.append(f"{label}: sockets must be Empty objects")
        if obj.name.startswith("socket_") and obj.name not in SOCKETS:
            errors.append(f"{label}: use a documented socket name")
        if obj.name.endswith("_COL") and obj.get("collision", "hull") not in KNOWN_COLLISIONS:
            errors.append(f"{label}: set collision to box, hull, mesh or decompose")
        mode = obj.get("mode")
        if mode is not None and (
            not isinstance(mode, str)
            or not mode.strip()
            or any(not name.strip() for name in mode.split(","))
        ):
            errors.append(f"{label}: mode must be a comma-separated list of non-empty names")
        resolution = obj.get("vhacd_resolution")
        if resolution is not None and (
            not isinstance(resolution, int) or not 16 <= resolution <= 256
        ):
            errors.append(f"{label}: vhacd_resolution must be an integer from 16 to 256")
        max_hulls = obj.get("vhacd_max_hulls")
        if max_hulls is not None and (
            not isinstance(max_hulls, int) or not 1 <= max_hulls <= 64
        ):
            errors.append(f"{label}: vhacd_max_hulls must be an integer from 1 to 64")
        mover = obj.get("mover")
        if mover is not None:
            action_name = isinstance(mover, str) and bool(mover.strip())
            definition = hasattr(mover, "keys") and all(
                key in mover for key in ("start", "end", "speed")
            )
            valid_speed = (
                definition
                and isinstance(mover["speed"], (int, float))
                and math.isfinite(mover["speed"])
                and mover["speed"] > 0
            )
            if not (action_name or valid_speed):
                errors.append(f"{label}: mover must name an action or define start, end and positive speed")
        if role == "mover" and not layouts:
            errors.append(f"{label}: add scene layouts before authoring mover poses")
    validate_layout_objects(scene, layouts, errors)
    return errors


def export_scene(output: Path) -> None:
    """Validate the active scene and write a GLB with object extras."""
    scene = bpy.context.scene
    errors = validate_scene(scene)
    if errors:
        raise ValueError("Export refused:\n" + "\n".join(f"- {error}" for error in errors))
    if not bpy.data.filepath:
        raise ValueError("Save the .blend file before exporting so asset IDs persist")
    source_id = asset_id(scene)
    layout_names = scene_layouts(scene, [])
    for obj in scene.objects:
        obj["source_asset_id"] = f"0x{source_id:016x}"
        obj["source_layouts"] = ",".join(layout_names)
    bpy.ops.wm.save_as_mainfile(filepath=bpy.data.filepath)
    output.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.export_scene.gltf(
        filepath=str(output), export_format="GLB", export_yup=True, export_apply=True,
        export_extras=True, export_cameras=False, export_lights=False,
        export_animations=True, export_skins=False, export_morph=False,
    )


class WORLDPIPELINE_PT_export(bpy.types.Panel):
    """Blender sidebar action for the canonical export path."""
    bl_label = "World Pipeline"
    bl_idname = "WORLDPIPELINE_PT_export"
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "World"

    def draw(self, context: bpy.types.Context) -> None:
        self.layout.operator("worldpipeline.export_scene")


class WORLDPIPELINE_OT_export(bpy.types.Operator):
    """Export the active scene to content/exported/<blend-name>.glb."""
    bl_idname = "worldpipeline.export_scene"
    bl_label = "Validate and Export GLB"

    def execute(self, context: bpy.types.Context) -> set[str]:
        blend = Path(bpy.data.filepath)
        output = blend.parent / "exported" / f"{blend.stem}.glb"
        try:
            export_scene(output)
        except (OSError, ValueError) as error:
            self.report({"ERROR"}, str(error))
            return {"CANCELLED"}
        self.report({"INFO"}, f"Exported {output}")
        return {"FINISHED"}


def register() -> None:
    bpy.utils.register_class(WORLDPIPELINE_OT_export)
    bpy.utils.register_class(WORLDPIPELINE_PT_export)


def unregister() -> None:
    bpy.utils.unregister_class(WORLDPIPELINE_PT_export)
    bpy.utils.unregister_class(WORLDPIPELINE_OT_export)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else sys.argv[1:]
    args = parser.parse_args(argv)
    export_scene(args.output)


if __name__ == "__main__":
    main()
