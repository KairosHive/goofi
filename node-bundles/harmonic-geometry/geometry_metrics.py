"""GeometryMetrics: Biotuner geometry measurements as one labeled ARRAY.

Missing measurements remain NaN. Use Harmonicity for musical measures."""

import json
import numpy as np
from biotuner.harmonic_geometry import GeometryData, geometry_metrics
import goofi
from goofi.geometry import decode


class GeometryMetrics(goofi.Node):
    """Measure geometry size, field structure, connectivity and method features."""

    TAGS = ["analysis"]
    INPUTS = {"input": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {"values": goofi.DataType.ARRAY}
    PARAMS = {"common": {"max_frequency": goofi.FloatParam(5.0, 0.0, 30.0)}}

    def process(self, input):
        table = decode(input.data, input.meta)
        coords = table["coordinates"]
        info = json.loads(table["info"])
        arrays = {k: table[k] for k in ("edges", "faces", "weights") if k in table}
        if "grid" in table:
            arrays["field_grid"] = table["grid"]
        geom = GeometryData(table["type"], coords, **arrays,
                            parameters=info.get("parameters", {}), metadata=info.get("metadata", {}))
        if len(coords) == 0:
            metrics = {"n_vertices": 0.0}
        else:
            metrics = geometry_metrics(geom)
        scalars = {str(k): np.float32(v) for k, v in metrics.items() if np.isscalar(v) and not isinstance(v, str)}
        names = sorted(scalars)
        meta = {"geometry_kind": str(info.get("metadata", {}).get("method", ""))}
        return {"values": (np.array([scalars[k] for k in names], dtype=np.float32),
                 {**meta, "channels": {"dim0": names}})}
