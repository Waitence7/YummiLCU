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

# Cover depth contract for preview geometry. Y decreases outward from the page
# block. Keeping these layers separated prevents z-fighting when the right half
# folds onto the left half in the closed pose.
BOTTOM_COVER_SURFACE_Y = -0.146
TOP_COVER_SURFACE_Y = -0.164
MIN_CLOSED_LAYER_GAP = 0.014


def validate_cover_depth_contract():
    gap = abs(TOP_COVER_SURFACE_Y - BOTTOM_COVER_SURFACE_Y)
    if gap < MIN_CLOSED_LAYER_GAP:
        raise RuntimeError(
            f"Cover layers are too close for the closed pose: {gap:.4f} < {MIN_CLOSED_LAYER_GAP:.4f}"
        )



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


def rounded_rect_points(center_x, half_w, half_h, inset_x, inset_z, radius, segments=8):
    """Sample one continuous rounded rectangle in the cover X/Z plane."""
    extent_x = half_w - inset_x
    extent_z = half_h - inset_z
    radius = min(radius, extent_x, extent_z)
    corners = [
        (center_x + extent_x - radius, extent_z - radius, 0.0),
        (center_x - extent_x + radius, extent_z - radius, math.pi / 2),
        (center_x - extent_x + radius, -extent_z + radius, math.pi),
        (center_x + extent_x - radius, -extent_z + radius, math.pi * 1.5),
    ]
    points = []
    for cx, cz, start_angle in corners:
        for index in range(segments):
            angle = start_angle + (math.pi / 2) * (index / segments)
            points.append((cx + math.cos(angle) * radius, cz + math.sin(angle) * radius))
    return points


def add_rounded_rail(
    collection,
    name,
    center_x,
    y,
    half_w,
    half_h,
    inset_x,
    inset_z,
    corner_radius,
    thickness,
    material,
):
    """Create a single continuous rail so corner pieces never overlap."""
    curve = bpy.data.curves.new(name, type="CURVE")
    curve.dimensions = "3D"
    curve.resolution_u = 2
    curve.bevel_depth = thickness
    curve.bevel_resolution = 3
    curve.fill_mode = "FULL"

    points = rounded_rect_points(
        center_x,
        half_w,
        half_h,
        inset_x,
        inset_z,
        corner_radius,
        segments=10,
    )
    spline = curve.splines.new("POLY")
    spline.points.add(len(points) - 1)
    for point, (x, z) in zip(spline.points, points):
        point.co = (x, y, z, 1.0)
    spline.use_cyclic_u = True

    obj = bpy.data.objects.new(name, curve)
    obj.data.materials.append(material)
    obj["yummi_preview_only"] = True
    obj["yummi_non_overlapping_rail"] = True
    collection.objects.link(obj)
    return obj


def add_cover_preview(
    collection,
    center_x,
    suffix,
    gold,
    gem_material,
    leather_material,
    *,
    is_top_cover,
):
    # Y is the cover depth axis. The old generator put both halves at exactly
    # -0.151, so the two covers and their ornaments became coplanar after the
    # right half folded onto the left. Give the bottom and top covers distinct
    # physical layers and keep every ornament farther outward than its panel.
    cover_surface_y = TOP_COVER_SURFACE_Y if is_top_cover else BOTTOM_COVER_SURFACE_Y
    half_w = 0.5
    half_h = 1.0

    panel_y = cover_surface_y - (0.010 if is_top_cover else 0.006)
    panel = add_box(
        collection,
        f"LeatherPanel_{suffix}",
        (center_x, panel_y, 0.0),
        (0.888, 0.018, 1.760),
        leather_material,
        bevel=0.020,
    )
    panel["yummi_cover_layer"] = "top" if is_top_cover else "bottom"

    # The lower cover keeps only a restrained outer rim. When the book closes,
    # it reads as the gilded lower-cover edge instead of duplicating the full
    # medallion directly underneath the top cover.
    outer_y = cover_surface_y - (0.030 if is_top_cover else 0.022)
    add_rounded_rail(
        collection,
        f"OuterRail_{suffix}",
        center_x,
        outer_y,
        half_w,
        half_h,
        0.052,
        0.094,
        0.055,
        0.010 if is_top_cover else 0.008,
        gold,
    )

    if not is_top_cover:
        return

    # A thinner inner rail gives the cover hierarchy without stacking thick
    # boxes at each corner. Its extra Y offset guarantees a real depth gap.
    add_rounded_rail(
        collection,
        f"InnerRail_{suffix}",
        center_x,
        cover_surface_y - 0.036,
        half_w,
        half_h,
        0.118,
        0.220,
        0.044,
        0.0065,
        gold,
    )

    # Keep the medallion slimmer and slightly smaller than the old torus. The
    # previous 0.016 tube and 1.69 vertical stretch crowded the gem and made the
    # ring appear to intersect itself from oblique camera angles.
    bpy.ops.mesh.primitive_torus_add(
        major_radius=0.225,
        minor_radius=0.009,
        major_segments=64,
        minor_segments=10,
        location=(center_x, cover_surface_y - 0.042, -0.015),
        rotation=(math.pi / 2, 0.0, 0.0),
    )
    medallion = bpy.context.active_object
    medallion.name = f"Medallion_{suffix}"
    medallion.scale = (1.0, 1.0, 1.42)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    medallion.data.materials.append(gold)
    medallion["yummi_preview_only"] = True
    medallion["yummi_cover_layer"] = "top-decoration"
    for c in list(medallion.users_collection):
        c.objects.unlink(medallion)
    collection.objects.link(medallion)

    # The gem is deliberately smaller and more embedded. It still protrudes
    # past the medallion but no longer clips through the ring at its widest arc.
    bpy.ops.mesh.primitive_uv_sphere_add(
        segments=48,
        ring_count=24,
        location=(center_x, cover_surface_y - 0.061, -0.015),
    )
    gem = bpy.context.active_object
    gem.name = f"Gem_{suffix}"
    gem.scale = (0.072, 0.032, 0.165)
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    gem.data.materials.append(gem_material)
    gem["yummi_preview_only"] = True
    gem["yummi_cover_layer"] = "top-gem"
    for c in list(gem.users_collection):
        c.objects.unlink(gem)
    collection.objects.link(gem)


validate_cover_depth_contract()

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
leather_material = make_preview_material("Yummi_Preview_Leather", (0.20, 0.025, 0.022), 0.05, 0.58)

# The left half becomes the lower cover and the right half folds over it as
# the visible top cover. Do not duplicate the full ornament on both layers.
add_cover_preview(
    preview, -0.5, "LeftCover", gold, gem_material, leather_material, is_top_cover=False
)
add_cover_preview(
    preview, 0.5, "RightCover", gold, gem_material, leather_material, is_top_cover=True
)

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
