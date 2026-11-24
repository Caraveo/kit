"""Bake evaluated meshes and surface materials into a portable Kit model."""

import hashlib
import json
import math
import pathlib
import struct
import sys

import bpy
from mathutils import Matrix

source, destination = sys.argv[sys.argv.index("--") + 1 :]
extension = pathlib.Path(source).suffix.lower()
directory = pathlib.Path(destination).parent
bpy.ops.wm.read_factory_settings(use_empty=True)

if extension in {".glb", ".gltf"}:
    result = bpy.ops.import_scene.gltf(filepath=source)
elif extension == ".fbx":
    if "fbx_import" in dir(bpy.ops.wm):
        result = bpy.ops.wm.fbx_import(filepath=source, use_anim=True)
    else:
        result = bpy.ops.import_scene.fbx(filepath=source, use_anim=True, use_image_search=False)
elif extension == ".usdz":
    result = bpy.ops.wm.usd_import(filepath=source)
elif extension == ".stl":
    result = bpy.ops.wm.stl_import(filepath=source)
elif extension == ".obj":
    result = bpy.ops.wm.obj_import(filepath=source)
else:
    raise ValueError(f"Unsupported model extension: {extension}")

scene = bpy.context.scene
if "FINISHED" not in result or not any(obj.type == "MESH" for obj in scene.objects):
    raise ValueError("The file contains no importable mesh geometry")

# Blender works in Z-up. Kit uses Y-up and a -Z forward axis.
axes = Matrix.Rotation(-math.pi / 2, 4, "X")
materials = []
material_slots = {}
textures = {}
warnings = []
frames = []
mesh_files = {}
clips = []
minimum = [math.inf] * 3
maximum = [-math.inf] * 3


def image_color(socket):
    """Read a base-color image optionally multiplied by a constant tint."""
    if socket is None:
        return None, [1, 1, 1]
    if not socket.is_linked:
        return None, list(socket.default_value[:3])
    node = socket.links[0].from_node
    if node.type == "TEX_IMAGE":
        return node.image, [1, 1, 1]
    if node.type == "REROUTE":
        return image_color(node.inputs[0])
    if node.type == "MIX" and node.data_type == "RGBA" and node.blend_type == "MULTIPLY":
        a, b = node.inputs[6:8]
        if a.is_linked and not b.is_linked:
            image, color = image_color(a)
            factor = node.inputs[0].default_value
            return image, [c * (1 + (tint - 1) * factor) for c, tint in zip(color, b.default_value)]
    return None, [1, 1, 1]


def alpha_properties(socket):
    if socket is None:
        return 1.0, False, 0.0
    if not socket.is_linked:
        return socket.default_value, socket.default_value < 0.999, 0.0
    node = socket.links[0].from_node
    if node.type == "MATH":
        if node.operation == "GREATER_THAN":
            factor, _, _ = alpha_properties(node.inputs[0])
            return factor, False, node.inputs[1].default_value
        if node.operation == "SUBTRACT" and node.inputs[1].is_linked:
            step = node.inputs[1].links[0].from_node
            if step.type == "MATH" and step.operation == "LESS_THAN":
                factor, _, _ = alpha_properties(step.inputs[0])
                return factor, False, step.inputs[1].default_value
        if node.operation == "MULTIPLY" and not node.inputs[1].is_linked:
            factor, _, cutoff = alpha_properties(node.inputs[0])
            return factor * node.inputs[1].default_value, not cutoff, cutoff
    return 1.0, True, 0.0


def material_slot(material):
    key = material.name_full if material else None
    if key in material_slots:
        return material_slots[key]
    color = [1, 1, 1, 1]
    texture = None
    blended = False
    unlit = False
    cutoff = 0.0
    double_sided = False
    roughness = 0.5
    metallic = 0.0
    emission = [0, 0, 0]
    if material:
        double_sided = not material.use_backface_culling
        color = list(material.diffuse_color)
        nodes = material.node_tree.nodes if material.use_nodes and material.node_tree else []
        output = next((node for node in nodes if node.type == "OUTPUT_MATERIAL" and node.is_active_output), None)
        surface = output.inputs["Surface"] if output else None
        shader = surface.links[0].from_node if surface and surface.is_linked else None
        principled = shader if shader and shader.type == "BSDF_PRINCIPLED" else next(
            (node for node in nodes if node.type == "BSDF_PRINCIPLED"), None
        )
        unlit = bool(shader and shader.type in {"EMISSION", "BACKGROUND"})
        color_input = principled.inputs["Base Color"] if principled else shader.inputs.get("Color") if unlit else None
        if color_input is not None:
            image, color[:3] = image_color(color_input)
            if image:
                if image.name not in textures:
                    filename = f"texture-{len(textures)}.ktex"
                    original = image.file_format
                    try:
                        image.file_format = "PNG"
                        image.save(filepath=str(directory / filename))
                    except RuntimeError:
                        (directory / filename).unlink(missing_ok=True)
                        warnings.append(f"Could not read base-color texture {image.name!r}; using the material color")
                        filename = None
                    finally:
                        image.file_format = original
                    textures[image.name] = filename
                texture = textures[image.name]
        if principled:
            color[3], blended, cutoff = alpha_properties(principled.inputs.get("Alpha"))
            roughness = principled.inputs["Roughness"].default_value
            metallic = principled.inputs["Metallic"].default_value
            strength = principled.inputs["Emission Strength"].default_value
            emission = [value * strength for value in principled.inputs["Emission Color"].default_value[:3]]

    slot = len(materials)
    material_slots[key] = slot
    materials.append(dict(color=color, texture=texture, blended=blended, unlit=unlit, cutoff=cutoff,
                          roughness=roughness, metallic=metallic, emission=emission, double_sided=double_sided))
    return slot


def sample(frame):
    scene.frame_set(math.floor(frame), subframe=frame % 1)
    depsgraph = bpy.context.evaluated_depsgraph_get()
    files = {}
    try:
        for instance in depsgraph.object_instances:
            obj = instance.object
            if obj.type != "MESH":
                continue
            transform = axes @ instance.matrix_world
            normal_matrix = transform.to_3x3().inverted_safe().transposed()
            reverse = transform.to_3x3().determinant() < 0
            mesh = obj.to_mesh()
            try:
                mesh.calc_loop_triangles()
                uv = mesh.uv_layers.active
                for triangle in mesh.loop_triangles:
                    slot = material_slot(mesh.materials[triangle.material_index]
                                         if triangle.material_index < len(mesh.materials) else None)
                    if slot not in files:
                        name = f"frame-{len(frames)}-{slot}.mesh"
                        files[slot] = (name, (directory / name).open("wb"), hashlib.sha256())
                    name, output, digest = files[slot]
                    loops = list(triangle.loops)
                    if reverse:
                        loops.reverse()
                    for index in loops:
                        loop = mesh.loops[index]
                        position = transform @ mesh.vertices[loop.vertex_index].co
                        normal = (normal_matrix @ mesh.corner_normals[index].vector).normalized()
                        texcoord = uv.data[index].uv if uv else (0, 0)
                        vertex = struct.pack("<8f", *position, *normal, texcoord[0], 1 - texcoord[1])
                        output.write(vertex)
                        digest.update(vertex)
                        for axis in range(3):
                            minimum[axis] = min(minimum[axis], position[axis])
                            maximum[axis] = max(maximum[axis], position[axis])
            finally:
                obj.to_mesh_clear()
    finally:
        for _, output, _ in files.values():
            output.close()
    frame_parts = {}
    for slot, (name, _, digest) in files.items():
        key = digest.digest()
        previous = mesh_files.setdefault(key, name)
        if previous != name:
            (directory / name).unlink()
        frame_parts[slot] = previous
    frames.append(frame_parts)


def animation_data():
    for obj in scene.objects:
        if obj.animation_data:
            yield obj.name, obj.animation_data
        keys = getattr(obj.data, "shape_keys", None)
        if keys and keys.animation_data:
            yield keys.name, keys.animation_data


def bake_clip(name, start, end):
    source_fps = scene.render.fps / scene.render.fps_base
    seconds = (end - start) / source_fps
    intervals = max(1, round(seconds * min(source_fps, 24))) if seconds > 0 else 0
    count = intervals + 1
    fps = intervals / seconds if intervals else source_fps
    first = len(frames)
    for i in range(count):
        sample(start + (end - start) * i / max(1, intervals))
    clips.append(dict(name=name, fps=fps, first=first, count=count))


animated = list(animation_data())
sample(scene.frame_start)

# The glTF importer stores animations as named NLA tracks, with one track per
# animated object or shape-key set. Enable matching tracks together per clip.
tracks = {}
if extension in {".glb", ".gltf"}:
    for _, owner in animated:
        for track in owner.nla_tracks:
            if track.strips:
                tracks.setdefault(track.name, []).append(track)

fbx_clips = {}
if extension == ".fbx":
    # FBX animation stacks are imported as separate actions per object.
    owners = sorted(animated, key=lambda item: len(item[0]), reverse=True)
    for action in bpy.data.actions:
        for owner_name, owner in owners:
            prefix = owner_name + "|"
            if action.name.startswith(prefix) and action.slots:
                name = action.name[len(prefix):].split("|", 1)[0]
                fbx_clips.setdefault(name, []).append((owner, action))
                break

if tracks:
    for _, owner in animated:
        owner.action = None
        for track in owner.nla_tracks:
            track.is_solo = False
            track.mute = True
    for name, group in tracks.items():
        for track in group:
            track.mute = False
        strips = [strip for track in group for strip in track.strips]
        bake_clip(name, min(strip.frame_start for strip in strips), max(strip.frame_end for strip in strips))
        for track in group:
            track.mute = True
elif fbx_clips:
    for name, group in fbx_clips.items():
        for _, owner in animated:
            owner.action = None
        for owner, action in group:
            owner.action = action
            owner.action_slot = action.slots[0]
        ranges = [tuple(action.frame_range) for _, action in group]
        bake_clip(name, min(start for start, _ in ranges), max(end for _, end in ranges))
else:
    ranges = [tuple(owner.action.frame_range) for _, owner in animated if owner.action]
    ranges += [(strip.frame_start, strip.frame_end)
               for _, owner in animated for track in owner.nla_tracks if not track.mute for strip in track.strips]
    if not ranges and extension == ".usdz" and scene.frame_end > scene.frame_start:
        # USD can animate mesh data through cache modifiers without an action.
        if animated or any(mod.type in {"MESH_SEQUENCE_CACHE", "TRANSFORM_CACHE"}
                           for obj in scene.objects for mod in obj.modifiers) or any(
                               constraint.type == "TRANSFORM_CACHE" for obj in scene.objects
                               for constraint in obj.constraints
                           ):
            ranges.append((scene.frame_start, scene.frame_end))
    if ranges:
        bake_clip("Default", min(start for start, _ in ranges), max(end for _, end in ranges))

if not materials or not any(frames):
    raise ValueError("The file contains no triangulated mesh geometry")
manifest = dict(materials=materials, frames=[[frame.get(i) for i in range(len(materials))] for frame in frames],
                clips=clips, bounds=[minimum, maximum], warnings=warnings)
pathlib.Path(destination).write_text(json.dumps(manifest, allow_nan=False), encoding="utf-8")
