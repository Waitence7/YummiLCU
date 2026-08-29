"""Validate and export the editable Yummi book back to the Agent runtime GLB.

Open book-return-v2.blend, edit YummiBook, then run this script from Blender.
Material names encode the runtime face roles and must remain unchanged.
"""

from pathlib import Path
import bpy

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "src" / "assets" / "book-return-v2.glb"

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

book = bpy.data.objects.get("YummiBook")
if book is None or book.type != "MESH":
    raise RuntimeError("YummiBook mesh is required")

actual = {slot.material.name for slot in book.material_slots if slot.material}
missing = [name for name in EXPECTED_MATERIALS if name not in actual]
if missing:
    raise RuntimeError(f"Missing required Yummi materials: {missing}")

bpy.ops.object.select_all(action="DESELECT")
book.select_set(True)
bpy.context.view_layer.objects.active = book
bpy.ops.object.transform_apply(location=False, rotation=True, scale=True)

bpy.ops.export_scene.gltf(
    filepath=str(OUTPUT),
    export_format="GLB",
    use_selection=True,
    export_materials="EXPORT",
    export_texcoords=True,
    export_normals=False,
    export_tangents=False,
    export_animations=False,
    export_cameras=False,
    export_lights=False,
)
print(f"Exported runtime model: {OUTPUT}")
