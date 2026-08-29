from pathlib import Path
import sys

import bpy
from mathutils import Vector


if "--" not in sys.argv or len(sys.argv[sys.argv.index("--") + 1 :]) != 1:
    raise RuntimeError("Expected: -- output.glb")
OUTPUT = Path(sys.argv[sys.argv.index("--") + 1]).resolve()

scene = bpy.context.scene
scene.frame_set(24)
source_collection = bpy.data.collections.get("01_Fixed_Detailed_Book")
if source_collection is None:
    raise RuntimeError("01_Fixed_Detailed_Book collection is required")

depsgraph = bpy.context.evaluated_depsgraph_get()
export_collection = bpy.data.collections.new("App_Detailed_Export")
scene.collection.children.link(export_collection)
copies = []

for source in source_collection.all_objects:
    if source.type != "MESH":
        continue
    evaluated = source.evaluated_get(depsgraph)
    mesh = bpy.data.meshes.new_from_object(
        evaluated,
        preserve_all_data_layers=True,
        depsgraph=depsgraph,
    )
    copy = bpy.data.objects.new(source.name + "_AppBake", mesh)
    copy.matrix_world = evaluated.matrix_world.copy()
    export_collection.objects.link(copy)
    copies.append(copy)

if not copies:
    raise RuntimeError("Reference collection contains no mesh objects")

bpy.ops.object.select_all(action="DESELECT")
for obj in copies:
    obj.select_set(True)
bpy.context.view_layer.objects.active = copies[0]
bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
bpy.ops.object.join()
book = bpy.context.active_object
book.name = "YummiDetailedBook"
book.data.name = "YummiDetailedBook"
book.animation_data_clear()

# Collapse repeated slots created by joining many objects while retaining the
# source material names and PBR factors for the WebGL detail shader.
slot_materials = [slot.material for slot in book.material_slots]
unique_materials = []
unique_by_name = {}
old_to_new = {}
for old_index, material in enumerate(slot_materials):
    if material is None:
        name = "Yummi Detail Default"
        material = bpy.data.materials.get(name) or bpy.data.materials.new(name)
        material.diffuse_color = (0.35, 0.08, 0.05, 1.0)
    name = material.name
    if name not in unique_by_name:
        unique_by_name[name] = len(unique_materials)
        unique_materials.append(material)
    old_to_new[old_index] = unique_by_name[name]

polygon_material_indices = [
    old_to_new.get(polygon.material_index, 0)
    for polygon in book.data.polygons
]
book.data.materials.clear()
for material in unique_materials:
    book.data.materials.append(material)
for polygon, material_index in zip(book.data.polygons, polygon_material_indices):
    polygon.material_index = material_index

# Normalize to the exact fully-closed dimensions produced by the existing
# page-fold shader: x=0.86, y=1.44. Keep a readable 0.30 clip-space thickness.
points = [vertex.co.copy() for vertex in book.data.vertices]
minimum = Vector((min(p.x for p in points), min(p.y for p in points), min(p.z for p in points)))
maximum = Vector((max(p.x for p in points), max(p.y for p in points), max(p.z for p in points)))
center = (minimum + maximum) * 0.5
size = maximum - minimum
target = Vector((0.86, 1.44, 0.30))
for vertex in book.data.vertices:
    vertex.co.x = (vertex.co.x - center.x) * target.x / size.x
    vertex.co.y = (vertex.co.y - center.y) * target.y / size.y
    vertex.co.z = (vertex.co.z - center.z) * target.z / size.z

book.data.validate(verbose=True)
book.data.update(calc_edges=True)
book["yummi_model"] = "book-return-v2-detailed-closed"
book["normalized_dimensions"] = list(target)
book["source_blend"] = "book-return-v2-fixed-reference.blend"

bpy.ops.object.select_all(action="DESELECT")
book.select_set(True)
bpy.context.view_layer.objects.active = book
OUTPUT.parent.mkdir(parents=True, exist_ok=True)
bpy.ops.export_scene.gltf(
    filepath=str(OUTPUT),
    export_format="GLB",
    use_selection=True,
    export_apply=True,
    export_materials="EXPORT",
    export_texcoords=False,
    export_normals=True,
    export_tangents=False,
    export_animations=False,
    export_cameras=False,
    export_lights=False,
)
print(f"OUTPUT={OUTPUT}")
print(f"VERTICES={len(book.data.vertices)}")
print(f"POLYGONS={len(book.data.polygons)}")
print(f"MATERIALS={[slot.material.name for slot in book.material_slots if slot.material]}")
