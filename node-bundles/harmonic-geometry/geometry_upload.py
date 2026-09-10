"""Pack geometry primitives for GeometryRender without rasterizing an image."""

import numpy as np
import goofi


class GeometryUpload(goofi.Node):
    """Convert geometry TABLEs to a bounded GPU primitive array.

    Each row has twelve scalars: three XYZ vertices, primitive type (1 point,
    2 line, 3 triangle), color position, and a reserved zero. Coordinates stay
    in their original domain. Fields use the existing GeometryView.field path.
    """

    INPUTS = {"input": goofi.InputSlot(goofi.DataType.TABLE, required=True)}
    OUTPUTS = {"primitives": goofi.DataType.ARRAY}
    PARAMS = {"common": {"max_frequency": goofi.FloatParam(8.0, 0.0, 30.0)}}

    def process(self, input):
        table = input.table
        kind = table["type"].text
        if kind in ("field_2d", "vector_field_2d"):
            raise ValueError("Fields use GeometryView.field and HarmonicInk; GeometryUpload accepts points, curves, graphs and meshes")
        item = table["coordinates"]
        parts = [item.table[str(i)].data for i in range(len(item.table))] if item.kind == "TABLE" else [item.data]
        rows = []
        for part in parts:
            part = np.asarray(part, dtype=np.float32)
            if not part.size:
                continue
            if part.ndim != 2 or part.shape[1] not in (2, 3) or not np.all(np.isfinite(part)):
                raise ValueError("GeometryUpload needs finite Nx2 or Nx3 coordinates")
            xyz = np.pad(part, ((0, 0), (0, 3-part.shape[1])))
            key = "faces" if kind == "mesh_3d" else "edges" if kind in ("graph", "tree") else None
            if key is not None:
                indices = np.asarray(table[key].data)
                width = 3 if key == "faces" else 2
                if indices.ndim != 2 or indices.shape[1] != width or not np.all(np.isfinite(indices)) or np.any(indices != np.floor(indices)) or np.any(indices < 0) or np.any(indices >= len(xyz)):
                    raise ValueError("Geometry connectivity contains invalid vertex indices")
                indices = indices.astype(int)
            elif kind.startswith("point_cloud"):
                width = 1
                indices = np.arange(len(xyz))[:, None]
            else:
                width = 2
                indices = np.column_stack((np.arange(max(len(xyz)-1, 0)), np.arange(1, len(xyz))))
            block = np.zeros((len(indices), 12), dtype=np.float32)
            for vertex in range(3):
                block[:, vertex*3:vertex*3+3] = xyz[indices[:, min(vertex, width-1)]]
            block[:, 9] = width
            rows.append(block)
        count = sum(len(row) for row in rows)
        if count > 4096:
            raise ValueError("GeometryRender supports at most 4096 primitives; reduce geometry points, resolution or depth")
        packed = np.vstack(rows) if count else np.zeros((1, 12), dtype=np.float32)
        if count:
            packed[:, 10] = np.linspace(0, 1, count)
        return {"primitives": (packed, {"primitive_count": count})}
