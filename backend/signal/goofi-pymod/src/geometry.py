"""Indexed geometry in one tiled ARRAY, shared by signal nodes and GPU readers.

Eight header texels precede vertices, part lengths, edges, faces, weights and
grids. Vertices occur once. Integer fields use base-1024 digit pairs so f16 GPU
uploads preserve indices exactly. The array has shape [pages, 256, 4]. Fields
store coverage in vertex alpha; metadata contains descriptions, not payloads.
"""

import json
import numpy as np

KINDS = ("curve_2d", "curve_3d", "point_cloud_2d", "point_cloud_3d", "polygon",
         "curve_set_2d", "curve_set_3d", "polygon_set", "graph", "tree",
         "mesh_3d", "field_2d", "vector_field_2d")
MAGIC = 7319
WIDTH = 256


def _digits(values):
    v = np.asarray(values)
    if not np.all(np.isfinite(v)) or np.any(v != np.floor(v)) or np.any(v < 0) or np.any(v >= 1024**2):
        raise ValueError("Geometry indices and counts must be integers below 1048576")
    return np.stack((v//1024, v % 1024), axis=-1).astype(np.float32)


def _integers(values):
    if not np.all(np.isfinite(values)) or np.any(values != np.floor(values)) or np.any(values < 0) or np.any(values >= 1024):
        raise ValueError("Invalid geometry integer digits")
    return (values[..., 0]*1024+values[..., 1]).astype(np.int64)


def encode(frame):
    kind = frame["type"]
    coords = frame["coordinates"]
    parts = []
    if isinstance(coords, list) or (isinstance(coords, np.ndarray) and coords.dtype == object):
        parts = [len(part) for part in coords]
        coords = np.concatenate(coords) if parts else np.empty((0, 2))
    coords = np.asarray(coords, dtype=np.float32)
    height = width = 0
    is_field = kind in ("field_2d", "vector_field_2d")
    if is_field:
        if coords.ndim != (2 if kind == "field_2d" else 3):
            raise ValueError("Invalid geometry field rank")
        height, width = coords.shape[:2]
        dim = 1 if coords.ndim == 2 else coords.shape[2]
        coords = coords.reshape(-1, dim)
    else:
        if coords.ndim != 2 or coords.shape[1] not in (2, 3):
            raise ValueError("Geometry coordinates need Nx2 or Nx3 values")
        dim = coords.shape[1]
    if dim not in (1, 2, 3) or np.any(np.isinf(coords)) or (not is_field and not np.all(np.isfinite(coords))):
        raise ValueError("Geometry has invalid coordinates")
    vertices = np.zeros((len(coords), 4), dtype=np.float32)
    vertices[:, :dim] = coords
    vertices[:, 3] = np.all(np.isfinite(coords), axis=1)
    if "coverage" in frame:
        cover = np.asarray(frame["coverage"])
        if not is_field or cover.shape != (height, width) or not np.all(np.isfinite(cover)) or np.any(cover < 0) or np.any(cover > 1):
            raise ValueError("Invalid geometry coverage")
        vertices[:, 3] *= cover.ravel()
    rows = [np.zeros((8, 4), dtype=np.float32), vertices]

    def append(values):
        values = np.asarray(values, dtype=np.float32).ravel()
        block = np.zeros(((len(values)+3)//4, 4), dtype=np.float32)
        block.ravel()[:len(values)] = values
        rows.append(block)
        return len(values)

    append(_digits(parts))
    counts = []
    for key, columns in (("edges", 2), ("faces", 3)):
        indices = np.asarray(frame.get(key, np.empty((0, columns))))
        if indices.ndim != 2 or indices.shape[1] != columns or np.any(indices >= len(coords)):
            raise ValueError("Invalid geometry connectivity")
        digits = _digits(indices).reshape(len(indices), columns*2)
        block = np.zeros((len(indices), 4 if columns == 2 else 8), dtype=np.float32)
        block[:, :columns*2] = digits
        append(block)
        counts.append(len(indices))
    weights = append(frame.get("weights", []))
    grids = frame.get("grid", ())
    grid_size = append(grids)
    count = sum(len(row) for row in rows)
    header = [MAGIC, KINDS.index(kind), dim, len(coords), height, width, len(parts),
              *counts, weights, len(grids), grid_size, count, 0, 0, 0]
    rows[0][:] = _digits(header).reshape(8, 4)
    result = np.zeros(((count+WIDTH-1)//WIDTH, WIDTH, 4), dtype=np.float32)
    result.reshape(-1, 4)[:count] = np.vstack(rows)
    info = frame.get("info", {})
    if isinstance(info, str):
        info = json.loads(info)
    meta = {"info": json.dumps(info), "geometry_kind": str(info.get("metadata", {}).get("method", ""))}
    decode(result, meta)
    return result, meta


def decode(array, meta):
    array = np.asarray(array, dtype=np.float32)
    if array.ndim != 3 or array.shape[1:] != (WIDTH, 4) or not len(array):
        raise ValueError("Expected a tiled geometry ARRAY [pages,256,4]")
    data = array.reshape(-1, 4)
    header = _integers(data[:8].reshape(16, 2))
    magic, kind, dim, count, height, width, parts, edges, faces, weights, grids, grid_size, total = header[:13]
    if magic != MAGIC or kind >= len(KINDS) or dim not in (1, 2, 3) or total < 8 or total > len(data):
        raise ValueError("Invalid geometry header")
    if np.any(data[total:] != 0) or np.any(header[13:] != 0):
        raise ValueError("Unexpected geometry payload")
    at = 8

    def take(size):
        nonlocal at
        rows = (size+3)//4
        if at+rows > total:
            raise ValueError("Truncated geometry section")
        section = data[at:at+rows].ravel()[:size]
        at += rows
        return section

    vertices = take(count*4).reshape(count, 4)
    coords = vertices[:, :dim]
    lengths = _integers(take(parts*2).reshape(parts, 2))
    is_field = KINDS[kind] in ("field_2d", "vector_field_2d")
    if np.any(np.isinf(coords)) or (not is_field and not np.all(np.isfinite(coords))):
        raise ValueError("Invalid geometry coordinates")
    frame = {"type": KINDS[kind], "coordinates": coords, "info": meta.get("info", "{}")}
    if is_field:
        if height*width != count or not height or not width or parts or dim != (1 if KINDS[kind] == "field_2d" else 2):
            raise ValueError("Invalid geometry field shape")
        frame["coordinates"] = coords.reshape((height, width) if dim == 1 else (height, width, dim))
        cover = vertices[:, 3]
        if not np.all(np.isfinite(cover)) or np.any(cover < 0) or np.any(cover > 1):
            raise ValueError("Invalid geometry coverage")
        frame["coverage"] = cover.reshape(height, width)
    elif height or width:
        raise ValueError("Unexpected geometry field dimensions")
    elif parts:
        if lengths.sum() != count:
            raise ValueError("Invalid geometry part lengths")
        frame["coordinates"] = np.split(coords, np.cumsum(lengths)[:-1])
    for key, size, columns in (("edges", edges, 2), ("faces", faces, 3)):
        section = take(size*(4 if columns == 2 else 8)).reshape(size, 4 if columns == 2 else 8)
        indices = _integers(section[:, :columns*2].reshape(size, columns, 2))
        if np.any(indices >= count):
            raise ValueError("Invalid geometry vertex index")
        if size:
            frame[key] = indices
    section = take(weights)
    if weights:
        frame["weights"] = section
    section = take(grid_size)
    if grid_size != grids*height*width:
        raise ValueError("Invalid geometry grid shape")
    if grids:
        frame["grid"] = tuple(section.reshape(grids, height, width))
    if at != total:
        raise ValueError("Unexpected geometry payload")
    return frame
