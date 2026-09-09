"""GeometryBlend: explicit morphs between two settled geometry frames.

Fields use Biotuner blend_fields. domain mode requires matching physical grids;
image mode registers both images to [0, 1]^2 and is a visual crossfade. Curves
are sampled by arc length, with 2D lifted to z=0. Meshes require identical faces.
Graphs and curve sets have no default correspondence: render them with two
GeometryView nodes and blend their images through graphics:Composite.
"""

import json
import numpy as np
from biotuner.harmonic_geometry import GeometryData, blend_fields
import goofi


def grid_of(table):
    if "grid" not in table:
        return None
    grid = table["grid"].table
    return tuple(grid[str(i)].data for i in range(len(grid)))


def arc_sample(points, count, align):
    points = np.asarray(points, dtype=np.float64)
    if points.ndim != 2 or points.shape[1] not in (2, 3) or not len(points) or not np.all(np.isfinite(points)):
        raise ValueError("Curve morph needs nonempty finite 2D or 3D curves")
    if points.shape[1] == 2:
        points = np.column_stack((points, np.zeros(len(points))))
    if align:
        center = (points.min(axis=0)+points.max(axis=0))/2
        points = (points-center)/max(np.max(np.ptp(points, axis=0))/2, 1e-12)
    distance = np.r_[0.0, np.cumsum(np.linalg.norm(np.diff(points, axis=0), axis=1))]
    distance, keep = np.unique(distance, return_index=True)
    at = np.linspace(0, distance[-1], count)
    return np.column_stack([np.interp(at, distance, points[keep, axis]) for axis in range(3)])


class GeometryBlend(goofi.Node):
    """Blend scalar fields, curves, or meshes with matching connectivity."""

    TAGS = ["transform"]
    INPUTS = {"a": goofi.InputSlot(goofi.DataType.TABLE, required=True),
              "b": goofi.InputSlot(goofi.DataType.TABLE, required=True),
              "mix": goofi.InputSlot(goofi.DataType.ARRAY, required=False)}
    OUTPUTS = {"geometry": goofi.DataType.TABLE, "field": goofi.DataType.ARRAY, "trajectory": goofi.DataType.ARRAY}
    PARAMS = {"blend": {
        "mix": goofi.FloatParam(0.0, 0.0, 1.0, doc="0 is A, 1 is B; wired mix overrides this value."),
        "space": goofi.StringParam("domain", ["domain", "image"], doc="Fields: domain checks physical grids; image blends normalized pictures."),
        "points": goofi.IntParam(512, 32, 4096, doc="Samples along each curve before interpolation."),
        "align": goofi.BoolParam(False, doc="Curves: center each endpoint and scale its largest span to 2 before blending."),
    }, "common": {"max_frequency": goofi.FloatParam(12.0, 0.0, 60.0)}}

    def process(self, a, b, mix=None):
        p = self.params.blend
        t = p.mix
        if mix is not None:
            v = np.asarray(mix.data).ravel()
            if len(v) != 1 or not np.isfinite(v[0]):
                raise ValueError("GeometryBlend mix needs one finite value")
            t = float(np.clip(v[0], 0, 1))
        ta, tb = a.table, b.table
        try:
            ka, kb = ta["type"].text, tb["type"].text
            ca, cb = ta["coordinates"].data, tb["coordinates"].data
        except (KeyError, TypeError) as e:
            raise ValueError("GeometryBlend needs two geometry frames with array coordinates") from e
        frame = {}
        field = np.empty((0, 0), dtype=np.float32)
        trajectory = np.empty((0, 0), dtype=np.float32)
        if ka == kb == "field_2d":
            if ca.ndim != 2 or ca.shape != cb.shape:
                raise ValueError("Field blend requires the same resolution; set both generator resolutions alike")
            ga, gb = grid_of(ta), grid_of(tb)
            if p.space == "image":
                ga = gb = np.meshgrid(np.linspace(0, 1, ca.shape[1]), np.linspace(0, 1, ca.shape[0]))
            valid_a, valid_b = np.isfinite(ca), np.isfinite(cb)
            aa = np.where(valid_a, ta["coverage"].data if "coverage" in ta else 1.0, 0.0)
            ab = np.where(valid_b, tb["coverage"].data if "coverage" in tb else 1.0, 0.0)
            if ga is not None and (len(ga) != 2 or any(x.shape != ca.shape for x in ga)):
                raise ValueError("A has an invalid field grid")
            if gb is not None and (len(gb) != 2 or any(x.shape != cb.shape for x in gb)):
                raise ValueError("B has an invalid field grid")
            result = blend_fields(GeometryData(ka, np.where(valid_a, ca, 0), field_grid=ga),
                                  GeometryData(kb, np.where(valid_b, cb, 0), field_grid=gb), t)
            field = result.coordinates.astype(np.float32)
            coverage = ((1-t)*aa + t*ab).astype(np.float32)
            field[coverage <= 0] = np.nan
            frame = {"type": "field_2d", "coordinates": field, "coverage": coverage}
            if result.field_grid is not None:
                frame["grid"] = {str(i): v.astype(np.float32) for i, v in enumerate(result.field_grid)}
        elif ka in ("curve_2d", "curve_3d") and kb in ("curve_2d", "curve_3d"):
            pa, pb = arc_sample(ca, p.points, p.align), arc_sample(cb, p.points, p.align)
            coords = ((1-t)*pa+t*pb).astype(np.float32)
            frame = {"type": "curve_3d", "coordinates": coords}
            trajectory = coords.T
        elif ka == kb == "mesh_3d":
            if ca.shape != cb.shape or "faces" not in ta or "faces" not in tb or not np.array_equal(ta["faces"].data, tb["faces"].data):
                raise ValueError("Mesh blend needs equal vertex counts and identical triangle indices")
            if not np.all(np.isfinite(ca)) or not np.all(np.isfinite(cb)):
                raise ValueError("Mesh coordinates must be finite")
            frame = {"type": ka, "coordinates": ((1-t)*ca+t*cb).astype(np.float32), "faces": ta["faces"]}
        else:
            raise ValueError(f"No correspondence for {ka} and {kb}; render both and blend their images with Composite")
        frame["info"] = json.dumps({"parameters": {"mix": t, "space": p.space},
                                    "metadata": {"kind": "blended", "method": "geometry_blend", "morph": t}})
        return {"geometry": (frame, {}), "field": (field, {}),
                "trajectory": (trajectory, {"channels": {"dim0": ["x", "y", "z"][:len(trajectory)]}})}
