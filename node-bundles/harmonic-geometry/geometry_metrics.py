"""GeometryMetrics: measurements from Biotuner's geometry_metrics, for cables.

The result depends on the geometry kind. Named scalar entries form a TABLE;
values is the same set as a labeled vector. Missing measurements remain NaN,
not zero. Use Harmonicity for the chord's musical measures.
"""

import json
import numpy as np
from biotuner.harmonic_geometry import GeometryData, geometry_metrics
import goofi


class GeometryMetrics(goofi.Node):
    """Measure geometry size, field structure, connectivity, and method-specific features."""

    TAGS = ["analysis"]
    INPUTS = {"input": goofi.InputSlot(goofi.DataType.TABLE, required=True)}
    OUTPUTS = {"metrics": goofi.DataType.TABLE, "values": goofi.DataType.ARRAY}
    PARAMS = {"common": {"max_frequency": goofi.FloatParam(5.0, 0.0, 30.0)}}

    def process(self, input):
        table = input.table
        if "type" not in table or "coordinates" not in table:
            raise ValueError("GeometryMetrics needs a geometry TABLE")
        item = table["coordinates"]
        coords = item.data if item.kind == "ARRAY" else [item.table[str(i)].data for i in range(len(item.table))]
        info = json.loads(table["info"].text) if "info" in table else {}
        arrays = {k: table[k].data
                  for k in ("edges", "faces", "weights") if k in table}
        for key in ("edges", "faces"):
            if key in arrays:
                raw = arrays[key]
                if not np.all(np.isfinite(raw)) or np.any(raw != np.floor(raw)) or np.any(raw < 0) or np.any(raw >= len(coords)):
                    raise ValueError(f"{key} contains an invalid vertex index")
                arrays[key] = raw.astype(np.int64)
        if "grid" in table:
            grid = table["grid"].table
            arrays["field_grid"] = tuple(grid[str(i)].data for i in range(len(grid)))
        geom = GeometryData(table["type"].text, coords, **arrays,
                            parameters=info.get("parameters", {}), metadata=info.get("metadata", {}))
        if len(coords) == 0:
            metrics = {"n_vertices": 0.0}
        else:
            metrics = geometry_metrics(geom)
        scalars = {str(k): np.float32(v) for k, v in metrics.items() if np.isscalar(v) and not isinstance(v, str)}
        names = sorted(scalars)
        meta = {"geometry_kind": str(info.get("metadata", {}).get("method", ""))}
        return {"metrics": (scalars, meta), "values": (np.array([scalars[k] for k in names], dtype=np.float32),
                 {**meta, "channels": {"dim0": names}})}
