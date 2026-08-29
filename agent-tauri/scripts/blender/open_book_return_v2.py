"""Blender helper for Yummi book-return-v2.

Usage:
  blender --python scripts/blender/open_book_return_v2.py

Imports the runtime GLB, adds non-exported preview details that mirror the
runtime shader, adds a hinge helper, and saves an editable .blend beside it.
"""

from pathlib import Path
import math
import bpy

ROOT = Path(__file__).resolve().parents[2]
GLB = ROOT / "src" / "assets" / "book-return-v2.glb"
BLEND = ROOT / "src" / "assets" / "book-return-v2.blend"

EXPECTED_MATERIALS = [
    "Yummi_Page_Front",
    "Yummi_Cover",
    "Yummi_Spine_Left",
    "Yummi_Spine_Right",
    "Yummi_Page_Top_Left",
    "Yummi_Page_Bottom_Left",
    "Yummi_Page_Top_Right",
    "Yummi_Page_Bottom_Right",
]


def make_preview_material(name, base_color, metallic, roughness):
    material = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    material.diffuse_color = (*base_color, 1.0)
    material.use_nodes = True
    bsdf = material.node_tree.nodes.get("Principled BSDF")
    if bsdf:
        bsdf.inputs["Base Color"].default_value = (*base_color, 1.0)
        bsdf.inputs["Metallic"].default_value = metallic
        bsdf.inputs["Roughness"].default_value = roughness
    material["yummi_preview_only"] = True
    return material


def add_box(collection, name, location, scale, material, bevel=0.006):
    bpy.ops.mesh.primitive_cube_add(size=1.0, location=location)
    obj = bpy.context.active_object
    obj.name = name
    obj.scale = scale
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    if bevel > 0:
        modifier = obj.modifiers.new("Preview bevel", "BEVEL")
        modifier.width = bevel
        modifier.segments = 3
    obj.data.materials.append(material)
    obj["yummi_preview_only"] = True
    for c in list(obj.users_collection):
        c.objects.unlink(obj)
    collection.objects.link(obj)
    return obj


def add_cover_preview(collection, center_x, suffix, gold, gem_material):
    cover_y = -0.151
    half_w = 0.5
    half_h = 1.0

    def frame(inset_x, inset_z, bar_x, bar_z, prefix):
        left = center_x - half_w + inset_x
        right = center_x + half_w - inset_x
        top = half_h - inset_z
        bottom = -half_h + inset_z
        add_box(
            collection,
            f"{prefix}_L_{suffix}",
            (left, cover_y, 0.0),
            (bar_x, 0.010, top - bottom),
            gold,
        )
        add_box(
            collection,
            f"{prefix}_R_{suffix}",
            (right, cover_y, 0.0),
            (bar_x, 0.010, top - bottom),
            gold,
        )
        add_box(
            collection,
            f"{prefix}_T_{suffix}",
            (center_x, cover_y, top),
            (right - left, 0.010, bar_z),
            gold,
        )
        add_box(
            collection,
            f"{prefix}_B_{suffix}",
            (center_x, cover_y, bottom),
            (right - left, 0.010, bar_z),
            gold,
        )

    frame(0.048, 0.096, 0.010, 0.018, "OuterRail")
    frame(0.108, 0.216, 0.007, 0.013, "InnerRail")

    bpy.ops.mesh.primitive_torus_add(
        major_radius=0.275,
        minor_radius=0.016,
        major_segments=64,
        minor_segments=12,
        location=(center_x, cover_y - 0.012, -0.02),
        rotation=(math.pi / 2, 0.0, 0.0),
    )
    medallion = bpy.context.active_object
    medallion.name = f"Medallion_{suffix}"
    medallion.scale = (1.0, 1.0, 1.69)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    medallion.data.materials.append(gold)
    medallion["yummi_preview_only"] = True
    for c in list(medallion.users_collection):
        c.objects.unlink(medallion)
    collection.objects.link(medallion)

    bpy.ops.mesh.primitive_uv_sphere_add(
        segments=48,
        ring_count=24,
        location=(center_x, cover_y - 0.038, -0.02),
    )
    gem = bpy.context.active_object
    gem.name = f"Gem_{suffix}"
    gem.scale = (0.097, 0.048, 0.280)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    gem.data.materials.append(gem_material)
    gem["yummi_preview_only"] = True
    for c in list(gem.users_collection):
        c.objects.unlink(gem)
    collection.objects.link(gem)


bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)

bpy.ops.import_scene.gltf(filepath=str(GLB))
book = bpy.data.objects.get("YummiBook")
if book is None:
    mesh_objects = [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
    if len(mesh_objects) != 1:
        raise RuntimeError("YummiBook mesh was not found")
    book = mesh_objects[0]
    book.name = "YummiBook"

book["yummi_model"] = "book-return-v2"
book["yummi_hinge_axis"] = "X=0"
book["yummi_runtime_front_material"] = "Yummi_Page_Front"
book["yummi_edit_note"] = (
    "Keep YummiBook and the Yummi_* material names; apply object transforms before GLB export."
)

actual = {slot.material.name for slot in book.material_slots if slot.material}
missing = [name for name in EXPECTED_MATERIALS if name not in actual]
if missing:
    raise RuntimeError(f"Missing required Yummi materials: {missing}")

preview = bpy.data.collections.new("Yummi_Runtime_Shader_Preview")
bpy.context.scene.collection.children.link(preview)
preview["yummi_preview_only"] = True

gold = make_preview_material("Yummi_Preview_Gold", (0.88, 0.56, 0.10), 0.72, 0.24)
gem_material = make_preview_material("Yummi_Preview_Gem", (0.015, 0.28, 0.95), 0.18, 0.12)

# The runtime shader maps each physical half of the underside to a complete cover.
add_cover_preview(preview, -0.5, "LeftCover", gold, gem_material)
add_cover_preview(preview, 0.5, "RightCover", gold, gem_material)

hinge = bpy.data.objects.new("Yummi_Hinge_X0", None)
hinge.empty_display_type = "PLAIN_AXES"
hinge.empty_display_size = 0.35
hinge.location = (0.0, 0.0, 0.0)
hinge["yummi_helper_only"] = True
bpy.context.collection.objects.link(hinge)

bpy.ops.object.select_all(action="DESELECT")
book.select_set(True)
bpy.context.view_layer.objects.active = book

bpy.ops.wm.save_as_mainfile(filepath=str(BLEND))
print(f"Saved editable Blender source: {BLEND}")
