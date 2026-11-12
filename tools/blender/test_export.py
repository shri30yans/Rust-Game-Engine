"""Headless Blender regression checks for persistent IDs, validation and GLB extras."""

import importlib.util
import json
import struct
import sys
import tempfile
from pathlib import Path

import bpy

sys.dont_write_bytecode = True

SCRIPT = Path(__file__).with_name("export.py")
SPEC = importlib.util.spec_from_file_location("world_pipeline_export", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("could not load Blender exporter")
EXPORT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(EXPORT)


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def glb_json(path: Path) -> dict:
    data = path.read_bytes()
    require(data[:4] == b"glTF", "export did not produce GLB")
    chunk_length, chunk_kind = struct.unpack_from("<II", data, 12)
    require(chunk_kind == 0x4E4F534A, "GLB first chunk is not JSON")
    return json.loads(data[20 : 20 + chunk_length])


def run() -> None:
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.unit_settings.system = "METRIC"
    scene.unit_settings.system = "NONE"
    unit_problems = EXPORT.validate_scene(scene)
    require(any("Metric" in problem for problem in unit_problems), "non-metric units were not reported")
    scene.unit_settings.system = "METRIC"
    scene["layouts"] = "open,Lockdown,,"
    require(
        any("lower_snake_case" in problem for problem in EXPORT.validate_scene(scene)),
        "invalid layout name was not reported",
    )
    scene["layouts"] = "open,open"
    require(
        any("duplicate names" in problem for problem in EXPORT.validate_scene(scene)),
        "duplicate layout name was not reported",
    )
    scene["layouts"] = "open,lockdown"
    mesh = bpy.data.meshes.new("fixture")
    mesh.from_pydata([(0, 0, 0), (1, 0, 0), (0, 1, 0)], [], [(0, 1, 2)])
    obj = bpy.data.objects.new("fixture_LOD4", mesh)
    scene.collection.objects.link(obj)
    obj["role"] = "not-a-role"
    obj["material"] = "unknown-surface"
    obj["collision"] = "unknown-shape"
    obj["team"] = 4
    obj["cue"] = " "
    obj["mode"] = ","
    obj["vhacd_resolution"] = 15
    obj["vhacd_max_hulls"] = 65
    obj["mover"] = " "
    obj.scale = (2.0, 1.0, 1.0)
    obj.rotation_euler[0] = 0.25
    socket = bpy.data.objects.new("socket_unknown", None)
    scene.collection.objects.link(socket)
    wrong_socket_mesh = bpy.data.meshes.new("socket_mesh")
    wrong_socket_mesh.from_pydata([(0, 0, 0), (1, 0, 0), (0, 1, 0)], [], [(0, 1, 2)])
    wrong_socket = bpy.data.objects.new("socket_muzzle", wrong_socket_mesh)
    scene.collection.objects.link(wrong_socket)
    problems = EXPORT.validate_scene(scene)
    require(any("role must be" in problem for problem in problems), "unknown role was not reported")
    require(any("material must" in problem for problem in problems), "unknown material was not reported")
    require(any("collision must" in problem for problem in problems), "unknown collision was not reported")
    require(any("team must" in problem for problem in problems), "invalid team was not reported")
    require(any("cue must" in problem for problem in problems), "empty cue was not reported")
    require(any("mode must" in problem for problem in problems), "invalid mode was not reported")
    require(any("vhacd_resolution" in problem for problem in problems), "invalid VHACD resolution was not reported")
    require(any("vhacd_max_hulls" in problem for problem in problems), "invalid hull limit was not reported")
    require(any("mover must" in problem for problem in problems), "invalid mover was not reported")
    require(any("_LOD0" in problem for problem in problems), "bad LOD suffix was not reported")
    require(any("documented socket" in problem for problem in problems), "unknown socket was not reported")
    require(any("sockets must be Empty" in problem for problem in problems), "mesh socket was not reported")
    require(any("apply scale" in problem for problem in problems), "unapplied scale was not reported")
    require(any("apply rotation" in problem for problem in problems), "unapplied rotation was not reported")
    mover = bpy.data.objects.new("door_a", None)
    scene.collection.objects.link(mover)
    mover["role"] = "mover"
    mover["mover"] = "door_motion"
    problems = EXPORT.validate_scene(scene)
    require(any("door_a@open" in problem for problem in problems), "missing mover pose was not reported")
    pose_open = bpy.data.objects.new("door_a@open", None)
    scene.collection.objects.link(pose_open)
    pose_open.parent = mover
    pose_extra = bpy.data.objects.new("door_a@secret", None)
    scene.collection.objects.link(pose_extra)
    pose_extra.parent = mover
    problems = EXPORT.validate_scene(scene)
    require(any("extra layout pose" in problem for problem in problems), "extra mover pose was not reported")
    scene.collection.objects.unlink(pose_extra)
    bpy.data.objects.remove(pose_extra)
    pose_mesh = bpy.data.meshes.new("pose_mesh")
    pose_mesh.from_pydata([(0, 0, 0), (1, 0, 0), (0, 1, 0)], [], [(0, 1, 2)])
    pose_wrong_type = bpy.data.objects.new("door_a@lockdown", pose_mesh)
    scene.collection.objects.link(pose_wrong_type)
    pose_wrong_type.parent = mover
    problems = EXPORT.validate_scene(scene)
    require(any("must be an Empty" in problem for problem in problems), "non-empty mover pose was not reported")
    switch = bpy.data.objects.new("alarm", None)
    scene.collection.objects.link(switch)
    switch["switch"] = "alarm"
    switch["layout_open"] = "on"
    problems = EXPORT.validate_scene(scene)
    require(any("layout_lockdown" in problem for problem in problems), "missing switch state was not reported")
    switch["layout_lockdown"] = "maybe"
    require(
        any("must be on or off" in problem for problem in EXPORT.validate_scene(scene)),
        "invalid switch state was not reported",
    )
    cue_object = bpy.data.objects.new("CUE_lockdown", None)
    scene.collection.objects.link(cue_object)
    cue_object["layout"] = "missing"
    cue_object["duration_s"] = 0.0
    problems = EXPORT.validate_scene(scene)
    require(any("layout must name" in problem for problem in problems), "unknown cue layout was not reported")
    require(any("duration_s" in problem for problem in problems), "invalid cue duration was not reported")
    centered_mesh = bpy.data.meshes.new("centered")
    centered_mesh.from_pydata(
        [(-1, -1, -1), (1, -1, -1), (0, 1, 1)], [], [(0, 1, 2)]
    )
    centered = bpy.data.objects.new("centered", centered_mesh)
    scene.collection.objects.link(centered)
    require(
        any("ground or attachment" in problem for problem in EXPORT.validate_scene(scene)),
        "origin away from a bounds face was not reported",
    )
    scene.collection.objects.unlink(centered)
    bpy.data.objects.remove(centered)
    bpy.data.meshes.remove(centered_mesh)
    collision_mesh = bpy.data.meshes.new("wall_collision")
    collision_mesh.from_pydata([(0, 0, 0), (1, 0, 0), (0, 1, 0)], [], [(0, 1, 2)])
    wall = bpy.data.objects.new("wall", None)
    scene.collection.objects.link(wall)
    collision = bpy.data.objects.new("wall_COL", collision_mesh)
    scene.collection.objects.link(collision)
    require(
        any("parent the _COL" in problem for problem in EXPORT.validate_scene(scene)),
        "unparented _COL mesh was not reported",
    )
    wrong_parent = bpy.data.objects.new("wrong_parent", None)
    scene.collection.objects.link(wrong_parent)
    collision.parent = wrong_parent
    require(
        any("parent the _COL" in problem for problem in EXPORT.validate_scene(scene)),
        "_COL mesh with the wrong parent was not reported",
    )
    collision.parent = wall
    require(
        not any("parent the _COL" in problem for problem in EXPORT.validate_scene(scene)),
        "parented _COL mesh was rejected",
    )
    scene.collection.objects.unlink(pose_wrong_type)
    bpy.data.objects.remove(pose_wrong_type)
    bpy.data.meshes.remove(pose_mesh)
    pose_lockdown = bpy.data.objects.new("door_a@lockdown", None)
    scene.collection.objects.link(pose_lockdown)
    pose_lockdown.parent = mover
    switch["layout_lockdown"] = "off"
    cue_object["layout"] = "lockdown"
    cue_object["duration_s"] = 0.75
    problems = EXPORT.validate_scene(scene)
    require(not any("door_a:" in problem for problem in problems), "complete mover layouts were rejected")
    require(not any("alarm:" in problem for problem in problems), "complete switch layouts were rejected")
    require(not any("CUE_lockdown:" in problem for problem in problems), "valid cue was rejected")

    obj["role"] = "static"
    del obj["material"]
    del obj["collision"]
    del obj["team"]
    del obj["cue"]
    del obj["mode"]
    del obj["vhacd_resolution"]
    del obj["vhacd_max_hulls"]
    del obj["mover"]
    obj.scale = (1.0, 1.0, 1.0)
    obj.rotation_euler = (0.0, 0.0, 0.0)
    obj.name = "fixture"
    scene.collection.objects.unlink(socket)
    scene.collection.objects.unlink(collision)
    bpy.data.objects.remove(collision)
    bpy.data.meshes.remove(collision_mesh)
    scene.collection.objects.unlink(wall)
    bpy.data.objects.remove(wall)
    scene.collection.objects.unlink(wrong_parent)
    bpy.data.objects.remove(wrong_parent)
    for pose in (pose_open, pose_lockdown):
        scene.collection.objects.unlink(pose)
        bpy.data.objects.remove(pose)
    scene.collection.objects.unlink(mover)
    bpy.data.objects.remove(mover)
    scene.collection.objects.unlink(switch)
    bpy.data.objects.remove(switch)
    scene.collection.objects.unlink(cue_object)
    bpy.data.objects.remove(cue_object)
    scene.collection.objects.unlink(wrong_socket)
    bpy.data.objects.remove(wrong_socket)
    bpy.data.meshes.remove(wrong_socket_mesh)
    EXPORT.validate_scene(scene)
    stable_id = obj["asset_id"]
    EXPORT.validate_scene(scene)
    require(obj["asset_id"] == stable_id, "object ID changed across validations")

    duplicate = obj.copy()
    duplicate.data = obj.data.copy()
    duplicate.name = "duplicate"
    scene.collection.objects.link(duplicate)
    EXPORT.validate_scene(scene)
    require(duplicate["asset_id"] != stable_id, "duplicate object ID was not reassigned")

    with tempfile.TemporaryDirectory(prefix="world-pipeline-export-") as root:
        blend = Path(root) / "fixture.blend"
        bpy.ops.wm.save_as_mainfile(filepath=str(blend))
        persisted_id = obj["asset_id"]
        bpy.ops.wm.open_mainfile(filepath=str(blend))
        obj = bpy.data.objects.get("fixture")
        require(obj is not None, "saved fixture object was missing")
        require(obj["asset_id"] == persisted_id, "object ID did not persist in the blend file")
        source_id = bpy.context.scene["asset_id"]
        target = Path(root) / "fixture.glb"
        EXPORT.export_scene(target)
        document = glb_json(target)
        nodes = document.get("nodes", [])
        extras = [node.get("extras", {}) for node in nodes]
        require(any("asset_id" in tags for tags in extras), "GLB omitted asset_id extras")
        require(
            all(tags.get("source_asset_id") == source_id for tags in extras),
            "GLB omitted or changed the source-file ID",
        )
        require(
            all(tags.get("source_layouts") == "open,lockdown" for tags in extras),
            "GLB omitted or changed scene layouts",
        )
        EXPORT.export_scene(target)
        require(obj["asset_id"] == persisted_id, "object ID changed across exports")
        require(bpy.context.scene["asset_id"] == source_id, "source ID changed across exports")


run()
print("world-pipeline Blender exporter tests passed")
