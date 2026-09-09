"""HarmonicTransport: sand, powder, tracer flow, or streaming on a supplied field.

Granular outputs an equilibrium density, not a particle simulation. Tracer and
Streaming output a velocity field. Wire flow to HarmonicFlow for persistent ink.
Native coordinates retain NaN domain masks; field/flow are finite shader arrays
with alpha in the fourth channel (scalar, scalar, scalar, coverage) or (u,v,0,coverage).
"""

import json
import numpy as np
from biotuner.harmonic_geometry import GeometryData, Granular, Tracer, Streaming
import goofi


class HarmonicTransport(goofi.Node):
    """Apply Biotuner transport operators to an existing scalar geometry field."""

    TAGS = ["transform", "image"]
    INPUTS = {"input": goofi.InputSlot(goofi.DataType.TABLE, required=True)}
    OUTPUTS = {"geometry": goofi.DataType.TABLE, "field": goofi.DataType.ARRAY, "flow": goofi.DataType.ARRAY}
    PARAMS = {"transport": {
        "method": goofi.StringParam("sand", ["sand", "particles", "tracer", "streaming"], doc="Equilibrium grains or a vector flow field."),
        "affinity": goofi.FloatParam(1.0, -2.0, 2.0, doc="Positive collects sand at nodes; negative collects powder at antinodes."),
        "temperature": goofi.FloatParam(0.035, 0.001, 2.0, doc="Lower values give sharper grain concentrations."),
        "potential": goofi.StringParam("displacement", ["displacement", "energy_gradient"], doc="Grain potential from field displacement or energy gradient."),
        "flow_kind": goofi.StringParam("mixed", ["gradient", "curl", "mixed"], doc="Flow along gradients, around level curves, or between them."),
        "mixing": goofi.FloatParam(0.15, 0.0, 1.0, doc="Mixed tracer flow: 0 is curl, 1 is gradient."),
        "viscosity": goofi.FloatParam(1.0, 0.01, 10.0, doc="Streaming viscosity."),
        "particles": goofi.IntParam(1500, 32, 8000, doc="Number of equilibrium samples; this is not an evolving particle state."),
        "seed": goofi.IntParam(7, 0, 65535, doc="Repeatable seed for particle sampling."),
    }, "common": {"max_frequency": goofi.FloatParam(10.0, 0.0, 30.0)}}

    def process(self, input):
        p, table = self.params.transport, input.table
        if "type" not in table or table["type"].text != "field_2d":
            raise ValueError("HarmonicTransport needs a scalar field_2d geometry")
        coords = np.asarray(table["coordinates"].data, dtype=np.float64)
        if coords.ndim != 2 or min(coords.shape) < 2 or max(coords.shape) > 512 or np.any(np.isinf(coords)):
            raise ValueError("Transport field must be a 2D grid of size 2..512 without infinities")
        grid = None
        if "grid" in table:
            entries = table["grid"].table
            grid = tuple(entries[str(i)].data for i in range(len(entries)))
            if len(grid) != 2 or any(x.shape != coords.shape for x in grid):
                raise ValueError("Transport grid must contain two arrays matching the field")
        else:
            grid = np.meshgrid(np.arange(coords.shape[1]), np.arange(coords.shape[0]))
        geom = GeometryData("field_2d", coords, field_grid=grid)
        if p.method in ("sand", "particles"):
            operator = Granular(affinity=p.affinity, temperature=p.temperature, field_kind=p.potential,
                                output_mode="particles" if p.method == "particles" else "density", n_particles=p.particles, seed=p.seed)
        elif p.method == "tracer":
            operator = Tracer(flow_kind=p.flow_kind, mixing=p.mixing)
        else:
            operator = Streaming(viscosity=p.viscosity)
        result = operator.respond(geom)
        if result.geom_type in ("field_2d", "vector_field_2d"):
            # Upstream flow operators fill masked cells with zero. Keep the source domain.
            result.coordinates = np.where(np.isfinite(coords)[..., None], result.coordinates, np.nan) if result.coordinates.ndim == 3 else np.where(np.isfinite(coords), result.coordinates, np.nan)
        frame = {"type": result.geom_type, "coordinates": np.asarray(result.coordinates, dtype=np.float32),
                 "info": json.dumps({"parameters": {"method": p.method, "affinity": p.affinity},
                                     "metadata": {"kind": p.method, "method": p.method}})}
        if result.field_grid is not None:
            frame["grid"] = {str(i): v.astype(np.float32) for i, v in enumerate(result.field_grid)}
        field = flow = np.empty((0, 0, 4), dtype=np.float32)
        values = np.asarray(result.coordinates)
        if result.geom_type in ("field_2d", "vector_field_2d"):
            valid = np.isfinite(values) if values.ndim == 2 else np.all(np.isfinite(values), axis=-1)
            coverage = table["coverage"].data if "coverage" in table else np.ones(coords.shape)
            if coverage.shape != coords.shape or not np.all(np.isfinite(coverage)):
                raise ValueError("Transport coverage must be finite and match the field")
            rgba = np.zeros((*values.shape[:2], 4), dtype=np.float32)
            rgba[..., 3] = valid * np.clip(coverage, 0, 1)
            frame["coverage"] = rgba[..., 3]
            if values.ndim == 2:
                rgba[..., :3] = np.nan_to_num(values, nan=0.0)[..., None]
                field = rgba
            else:
                rgba[..., :2] = np.nan_to_num(values, nan=0.0)
                flow = rgba
        return {"geometry": (frame, {}), "field": (field, {}), "flow": (flow, {})}
