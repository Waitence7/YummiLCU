# Yummi book-return-v2 Blender workflow

`src/assets/book-return-v2.glb` is the runtime geometry for the close animation and is directly importable in Blender.

## First editable .blend

Open Blender and run:

```bash
blender --python scripts/blender/open_book_return_v2.py
```

This imports the checked-in GLB, creates a `Yummi_Hinge_X0` helper at the runtime hinge, and saves:

```text
src/assets/book-return-v2.blend
```

## Editing rules

- Keep the mesh object name `YummiBook`.
- Keep the eight `Yummi_*` material names. The Agent uses them to recover the runtime face roles.
- The page/cover fold hinge is local `X = 0`.
- Keep a UV map on all faces. `Yummi_Page_Front` receives the live Agent window snapshot.
- The front and cover are intentionally subdivided (32 x 26) so the shader can fold them smoothly.
- Object rotation/scale should be applied before export.
- Preview cover geometry intentionally uses separate Y-depth layers for the lower and upper covers. Do not make them coplanar; the separation prevents z-fighting in the closed pose.
- The lower cover carries only a restrained outer rail. The upper/right cover owns the inner rail, medallion, and gem so ornaments do not duplicate when the book closes.
- Gold frame rails are generated as continuous rounded curves rather than four intersecting bars; keep that topology when changing the frame proportions.

You can change proportions, thickness, vertices, UVs, bevels, or silhouette as long as the face-role materials remain assigned.

## Export back to the Agent

With `book-return-v2.blend` open, run:

```bash
blender --python scripts/blender/export_book_return_v2.py
```

The script validates the material roles, applies rotation/scale, and overwrites:

```text
src/assets/book-return-v2.glb
```

The Agent loads this GLB at runtime. If loading fails, it falls back to the old procedural mesh.
