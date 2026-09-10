"""Indexed geometry fixtures for real CPU and GPU sessions."""
import numpy as np
import goofi
from goofi.geometry import encode, decode


class GeometryArrayFixture(goofi.Node):
    OUTPUTS = {"geometry": goofi.DataType.ARRAY}
    PARAMS = {"fixture": {"kind": goofi.StringParam("mesh", ["mesh", "parts", "field", "invalid"])},
              "common": {"autotrigger": goofi.BoolParam(True), "max_frequency": goofi.FloatParam(10.0, 0.0, 30.0)}}

    def process(self):
        kind = self.params.fixture.kind
        if kind == "parts":
            frame = {"type": "curve_set_2d", "coordinates": [np.array([[-.8, -.7], [-.8, .7]]), np.array([[.8, -.7], [.8, .7]])]}
        elif kind == "field":
            y, x = np.mgrid[:64, :64]/63.0
            field = x-y
            field[:8, :8] = np.nan
            frame = {"type": "field_2d", "coordinates": field, "grid": (x, y)}
        else:
            vertices = np.zeros((4098, 3), dtype=np.float32)
            vertices[-3:] = [[-.7, -.7, 0], [0, .7, 0], [.7, -.7, 0]]
            frame = {"type": "mesh_3d", "coordinates": vertices, "faces": np.array([[4095, 4096, 4097]])}
        frame["info"] = {"metadata": {"method": kind}}
        values, meta = encode(frame)
        decoded = decode(values, meta)
        if "faces" in frame:
            assert np.array_equal(decoded["faces"], frame["faces"])
            reduced = values.astype(np.float16).astype(np.float32)
            assert np.array_equal(decode(reduced, meta)["faces"], frame["faces"])
        if kind == "invalid":
            # First face starts after eight header texels and 4098 vertices.
            values.reshape(-1, 4)[4106, :2] = [0, 9999]
        return {"geometry": (values, meta)}
