#!/usr/bin/env python3
"""Generate the Blender-editable base mesh used by Yummi book-return-v2.

The output is a standard glTF 2.0 GLB. Blender can import it directly.
Keep the material names when exporting so the Agent can recover the face roles.
"""

from __future__ import annotations

import json
import math
import struct
from pathlib import Path
from typing import Iterable

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "src" / "assets" / "book-return-v2.glb"

COLUMNS = 32
ROWS = 26
HALF_WIDTH = 1.0
HALF_HEIGHT = 1.0
HALF_DEPTH = 0.135

MATERIALS = [
    ("Yummi_Page_Front", [0.93, 0.94, 0.95, 1.0], 0.88, 0.0),
    ("Yummi_Cover", [0.24, 0.035, 0.026, 1.0], 0.62, 0.0),
    ("Yummi_Spine_Left", [0.28, 0.055, 0.035, 1.0], 0.68, 0.0),
    ("Yummi_Spine_Right", [0.72, 0.45, 0.09, 1.0], 0.54, 0.12),
    ("Yummi_Page_Top_Left", [0.72, 0.53, 0.18, 1.0], 0.78, 0.0),
    ("Yummi_Page_Bottom_Left", [0.66, 0.47, 0.15, 1.0], 0.80, 0.0),
    ("Yummi_Page_Top_Right", [0.76, 0.56, 0.19, 1.0], 0.78, 0.0),
    ("Yummi_Page_Bottom_Right", [0.69, 0.49, 0.16, 1.0], 0.80, 0.0),
]


def subdivided_plane(z: float, *, reverse: bool = False):
    positions: list[tuple[float, float, float]] = []
    uvs: list[tuple[float, float]] = []
    indices: list[int] = []
    vertex = 0

    for row in range(ROWS):
        v0 = row / ROWS
        v1 = (row + 1) / ROWS
        for col in range(COLUMNS):
            u0 = col / COLUMNS
            u1 = (col + 1) / COLUMNS
            quad = [(u0, v0), (u0, v1), (u1, v0), (u1, v1)]
            for u, v in quad:
                positions.append(((u * 2 - 1) * HALF_WIDTH, (1 - v * 2) * HALF_HEIGHT, z))
                uvs.append((u, v))
            if reverse:
                indices.extend((vertex, vertex + 2, vertex + 1, vertex + 2, vertex + 3, vertex + 1))
            else:
                indices.extend((vertex, vertex + 1, vertex + 2, vertex + 2, vertex + 1, vertex + 3))
            vertex += 4

    return positions, uvs, indices


def quad(corners: Iterable[tuple[float, float, float]]):
    positions = list(corners)
    uvs = [(0.0, 0.0), (0.0, 1.0), (1.0, 0.0), (1.0, 1.0)]
    indices = [0, 1, 2, 2, 1, 3]
    return positions, uvs, indices


def build_primitives():
    front = subdivided_plane(-HALF_DEPTH)
    back = subdivided_plane(HALF_DEPTH, reverse=True)
    left = quad([
        (-HALF_WIDTH, HALF_HEIGHT, HALF_DEPTH),
        (-HALF_WIDTH, -HALF_HEIGHT, HALF_DEPTH),
        (-HALF_WIDTH, HALF_HEIGHT, -HALF_DEPTH),
        (-HALF_WIDTH, -HALF_HEIGHT, -HALF_DEPTH),
    ])
    right = quad([
        (HALF_WIDTH, HALF_HEIGHT, -HALF_DEPTH),
        (HALF_WIDTH, -HALF_HEIGHT, -HALF_DEPTH),
        (HALF_WIDTH, HALF_HEIGHT, HALF_DEPTH),
        (HALF_WIDTH, -HALF_HEIGHT, HALF_DEPTH),
    ])
    top_left = quad([
        (-HALF_WIDTH, HALF_HEIGHT, HALF_DEPTH),
        (-HALF_WIDTH, HALF_HEIGHT, -HALF_DEPTH),
        (0.0, HALF_HEIGHT, HALF_DEPTH),
        (0.0, HALF_HEIGHT, -HALF_DEPTH),
    ])
    bottom_left = quad([
        (-HALF_WIDTH, -HALF_HEIGHT, -HALF_DEPTH),
        (-HALF_WIDTH, -HALF_HEIGHT, HALF_DEPTH),
        (0.0, -HALF_HEIGHT, -HALF_DEPTH),
        (0.0, -HALF_HEIGHT, HALF_DEPTH),
    ])
    top_right = quad([
        (0.0, HALF_HEIGHT, HALF_DEPTH),
        (0.0, HALF_HEIGHT, -HALF_DEPTH),
        (HALF_WIDTH, HALF_HEIGHT, HALF_DEPTH),
        (HALF_WIDTH, HALF_HEIGHT, -HALF_DEPTH),
    ])
    bottom_right = quad([
        (0.0, -HALF_HEIGHT, -HALF_DEPTH),
        (0.0, -HALF_HEIGHT, HALF_DEPTH),
        (HALF_WIDTH, -HALF_HEIGHT, -HALF_DEPTH),
        (HALF_WIDTH, -HALF_HEIGHT, HALF_DEPTH),
    ])
    return [front, back, left, right, top_left, bottom_left, top_right, bottom_right]


def pad4(data: bytes, pad: bytes = b"\x00") -> bytes:
    return data + pad * ((4 - len(data) % 4) % 4)


def pack_f32(values: Iterable[float]) -> bytes:
    values = list(values)
    return struct.pack("<" + "f" * len(values), *values)


def pack_u16(values: Iterable[int]) -> bytes:
    values = list(values)
    return struct.pack("<" + "H" * len(values), *values)


def add_blob(binary: bytearray, payload: bytes) -> tuple[int, int]:
    while len(binary) % 4:
        binary.append(0)
    offset = len(binary)
    binary.extend(payload)
    return offset, len(payload)


def minmax_vec3(values: list[tuple[float, float, float]]):
    return (
        [min(v[i] for v in values) for i in range(3)],
        [max(v[i] for v in values) for i in range(3)],
    )


def main() -> None:
    primitives = build_primitives()
    binary = bytearray()
    buffer_views = []
    accessors = []
    gltf_primitives = []

    def make_view(payload: bytes, target: int) -> int:
        offset, length = add_blob(binary, payload)
        idx = len(buffer_views)
        buffer_views.append({
            "buffer": 0,
            "byteOffset": offset,
            "byteLength": length,
            "target": target,
        })
        return idx

    for face_id, (positions, uvs, indices) in enumerate(primitives):
        pos_flat = [value for vertex in positions for value in vertex]
        uv_flat = [value for uv in uvs for value in uv]

        pos_view = make_view(pack_f32(pos_flat), 34962)
        uv_view = make_view(pack_f32(uv_flat), 34962)
        idx_view = make_view(pack_u16(indices), 34963)

        pos_min, pos_max = minmax_vec3(positions)
        pos_accessor = len(accessors)
        accessors.append({
            "bufferView": pos_view,
            "componentType": 5126,
            "count": len(positions),
            "type": "VEC3",
            "min": pos_min,
            "max": pos_max,
        })
        uv_accessor = len(accessors)
        accessors.append({
            "bufferView": uv_view,
            "componentType": 5126,
            "count": len(uvs),
            "type": "VEC2",
            "min": [0.0, 0.0],
            "max": [1.0, 1.0],
        })
        idx_accessor = len(accessors)
        accessors.append({
            "bufferView": idx_view,
            "componentType": 5123,
            "count": len(indices),
            "type": "SCALAR",
            "min": [min(indices)],
            "max": [max(indices)],
        })

        gltf_primitives.append({
            "attributes": {
                "POSITION": pos_accessor,
                "TEXCOORD_0": uv_accessor,
            },
            "indices": idx_accessor,
            "material": face_id,
            "mode": 4,
            "extras": {
                "yummiBookFace": face_id,
            },
        })

    materials = []
    for name, base_color, roughness, metallic in MATERIALS:
        materials.append({
            "name": name,
            "pbrMetallicRoughness": {
                "baseColorFactor": base_color,
                "metallicFactor": metallic,
                "roughnessFactor": roughness,
            },
            "doubleSided": True,
        })

    gltf = {
        "asset": {
            "version": "2.0",
            "generator": "Yummi book-return-v2 generator",
            "extras": {
                "yummiModel": "book-return-v2",
                "editableInBlender": True,
                "dimensions": [HALF_WIDTH * 2, HALF_HEIGHT * 2, HALF_DEPTH * 2],
            },
        },
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": "YummiBook"}],
        "meshes": [{
            "name": "YummiBook",
            "primitives": gltf_primitives,
            "extras": {
                "yummiFaceMaterials": [material[0] for material in MATERIALS],
            },
        }],
        "materials": materials,
        "buffers": [{"byteLength": len(binary)}],
        "bufferViews": buffer_views,
        "accessors": accessors,
    }

    json_chunk = pad4(json.dumps(gltf, separators=(",", ":"), ensure_ascii=False).encode("utf-8"), b" ")
    bin_chunk = pad4(bytes(binary))
    total_length = 12 + 8 + len(json_chunk) + 8 + len(bin_chunk)

    output = bytearray()
    output.extend(struct.pack("<III", 0x46546C67, 2, total_length))
    output.extend(struct.pack("<II", len(json_chunk), 0x4E4F534A))
    output.extend(json_chunk)
    output.extend(struct.pack("<II", len(bin_chunk), 0x004E4942))
    output.extend(bin_chunk)

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_bytes(output)
    print(f"wrote {OUTPUT} ({len(output)} bytes)")


if __name__ == "__main__":
    main()
