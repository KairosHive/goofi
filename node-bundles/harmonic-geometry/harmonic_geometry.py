"""HarmonicGeometry: Biotuner generators behind one indexed geometry ARRAY.

Coordinates, connectivity, field grids and masks each occur once in the array.
GeometryView, GeometryBlend, GeometryMetrics, HarmonicTransport and GPU readers
consume this shared format. Descriptive parameters remain in metadata.

Closed curves, integer modes, graphs and fractals can change discretely.
Use lateral/rotary/trace_3d for fixed-duration tuning motion; use GeometryBlend
between fixed endpoint geometries for integer or topology changes."""

import json
import math
from fractions import Fraction
import numpy as np
from biotuner.harmonic_input import HarmonicInput
from biotuner import harmonic_geometry as hg
import goofi
from goofi.geometry import encode


METHODS = [
    "compound", "lateral", "rotary", "trace_3d", "closed_2d", "closed_3d", "pairwise",
    "rose", "epicycloid", "hypocycloid", "tuning_circle", "times_table", "interval_graph",
    "chord_graph", "consonance_polygon", "stern_brocot", "continued_fraction", "farey",
    "subharmonic_tree", "ifs", "lsystem", "recursive_polygon", "pitch_lattice",
    "tube", "knot", "torus", "sphere", "cylinder", "point_cloud", "tree_3d", "polyhedron",
    "plate", "pairwise_plate", "symmetric_plate", "triple_plate", "circular_plate", "elastic",
    "harmonic_field", "quasicrystal", "wave_lattice", "vortex", "sources", "acoustic",
    "faraday", "spherical_field", "spherical_mesh",
]


def json_value(value):
    if isinstance(value, np.ndarray):
        return value.tolist()
    if isinstance(value, np.generic):
        return value.item()
    if isinstance(value, Fraction):
        return float(value)
    raise TypeError(f"Cannot encode geometry metadata {type(value).__name__}")




class HarmonicGeometry(goofi.Node):
    """Curves, graphs, fractals, meshes, plates and wave fields from one chord.

    Coordinates retain their physical domain. The sole geometry ARRAY carries the selected method and its connectivity.
    Changing methods replaces the complete frame.
    """

    TAGS = ["transform", "image"]
    INPUTS = {"input": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {"geometry": goofi.DataType.ARRAY}
    PARAMS = {
        "geometry": {
            "method": goofi.StringParam("trace_3d", METHODS, doc="Biotuner geometry method. See the cookbook for continuous and discrete methods."),
            "points": goofi.IntParam(512, 32, 4096, doc="Curve or point-cloud sample count. More points cost more CPU and transport."),
            "resolution": goofi.IntParam(96, 16, 256, doc="Field side length; meshes use at most 64 samples per side."),
            "denominator": goofi.IntParam(8, 1, 16, doc="Largest rational denominator for closed curves, knots, surfaces, and integer structure."),
            "component": goofi.IntParam(1, 0, 31, doc="Ratio selected by single-ratio curves, counting from zero."),
        },
        "trace": {
            "duration": goofi.FloatParam(8.0, 0.25, 32.0, doc="Fixed trace duration in relative-frequency seconds; compound uses its integer floor as periods."),
            "rotation": goofi.FloatParam(0.08, -2.0, 2.0, doc="Rotary harmonograph rotation in turns per relative second."),
            "phase": goofi.FloatParam(1.57, -12.57, 12.57, doc="Phase offset for closed_2d and pairwise drawings, in radians."),
        },
        "structure": {
            "depth": goofi.IntParam(3, 1, 5, doc="Recursion depth; methods reject requests above their work budget."),
            "order": goofi.IntParam(5, 2, 12, doc="Farey order, polygon sides, quasicrystal symmetry, or wave direction count."),
            "seed": goofi.IntParam(7, 0, 65535, doc="Repeatable random seed for IFS and wave patterns."),
            "contraction": goofi.StringParam("ratio_inverse", ["ratio_inverse", "log_ratio", "fixed_half"], doc="Harmonic IFS contraction rule."),
            "circle_mode": goofi.StringParam("ratio", ["ratio", "pitch_class", "integer"], doc="How ratios drive times-table multipliers."),
        },
        "surface": {
            "tube_radius": goofi.FloatParam(0.06, 0.005, 0.3, doc="Radius of a Lissajous tube or knot tube."),
            "tube_sides": goofi.IntParam(8, 3, 16, doc="Cross-section sides; controls mesh cost."),
            "solid": goofi.StringParam("icosahedron", ["tetrahedron", "cube", "icosahedron"], doc="Base solid for recursive polyhedra."),
        },
        "field": {
            "extent": goofi.FloatParam(1.5, 0.1, 8.0, doc="Half-width of open wave domains."),
            "period": goofi.FloatParam(0.7, 0.05, 4.0, doc="Base wavelength/period for interference fields."),
            "output": goofi.StringParam("real", ["real", "amplitude", "intensity"], doc="Open-wave scalar: signed real field, magnitude, or squared magnitude."),
            "max_mode": goofi.IntParam(12, 1, 24, doc="Upper bound for plate and spherical mode indices."),
            "strategy": goofi.StringParam("best_simple", ["stern_brocot", "continued_fraction", "rounded", "best_simple"], doc="Ratio-to-plate-mode mapping."),
            "symmetry": goofi.StringParam("none", ["none", "d4_max", "d4_sum"], doc="Square symmetry for pairwise/triple plate fields."),
            "anisotropy": goofi.FloatParam(1.7, 0.1, 8.0, doc="Elastic plate stiffness ratio."),
            "axis": goofi.FloatParam(0.3, -3.14, 3.14, doc="Elastic stiffness direction in radians."),
            "faraday_pattern": goofi.StringParam("hexagonal", ["stripe", "square", "hexagonal", "twelve_fold", "random"], doc="Faraday pattern-selection regime."),
            "viscosity": goofi.FloatParam(0.002, 0.0, 0.1, doc="Faraday short-wave damping."),
        },
        "common": {"max_frequency": goofi.FloatParam(8.0, 0.0, 30.0)},
    }

    def process(self, input):
        g, tr, st, sf, f = self.params.geometry, self.params.trace, self.params.structure, self.params.surface, self.params.field
        if not (32 <= g.points <= 4096 and 16 <= g.resolution <= 256 and 1 <= g.denominator <= 16
                and 1 <= st.depth <= 5 and 2 <= st.order <= 12 and 3 <= sf.tube_sides <= 16
                and 0.25 <= tr.duration <= 32 and 1 <= f.max_mode <= 24):
            raise ValueError("Geometry sampling, mode, or recursion setting is outside its supported range")
        values = np.asarray(input.data, dtype=np.float64)
        if values.ndim != 2 or values.shape[0] != 4:
            raise ValueError("HarmonicGeometry needs a [4,N] harmonic ARRAY")
        r, a, ph, damp = values
        if r.ndim != 1 or any(v.shape != r.shape for v in (a, ph, damp)) or len(r) > 32:
            raise ValueError("Harmonic components must be aligned vectors with at most 32 entries")
        if not all(np.all(np.isfinite(v)) for v in (r, a, ph, damp)) or np.any(r <= 0) or np.any(a < 0) or np.any(damp < 0):
            raise ValueError("Harmonic components must be finite, with positive ratios and nonnegative amplitudes/damping")
        m, n, res = g.method, g.points, g.resolution
        if not r.size or not np.any(a > 0):
            geom = hg.GeometryData("point_cloud_3d", np.empty((0, 3)), metadata={"kind": m, "empty": True})
        else:
            discrete = m in ("closed_2d", "closed_3d", "pairwise", "rose", "epicycloid", "hypocycloid",
                "tuning_circle", "times_table", "interval_graph", "chord_graph", "consonance_polygon",
                "stern_brocot", "continued_fraction", "farey", "subharmonic_tree", "ifs", "lsystem",
                "recursive_polygon", "pitch_lattice", "knot", "torus", "sphere", "cylinder", "tree_3d",
                "polyhedron", "pairwise_plate", "symmetric_plate", "triple_plate", "vortex")
            if discrete:
                active = a > 0
                r, a, ph, damp = (v[active] for v in (r, a, ph, damp))
            h = HarmonicInput(peaks=r.tolist(), amplitudes=a.tolist(), phases=ph.tolist(), damping=damp.tolist(),
                              equave=float(input.meta.get("equave", 2.0)))
            rational = [Fraction(float(v)).limit_denominator(g.denominator) for v in r]
            needs_rational = discrete or m in ("plate", "circular_plate", "spherical_field", "spherical_mesh")
            if needs_rational and any(v <= 0 for v in rational):
                raise ValueError("Ratio is too small for this denominator; increase geometry/denominator")
            rh = HarmonicInput(ratios=rational, amplitudes=a.tolist(), phases=ph.tolist(), damping=damp.tolist(), equave=h.equave) if needs_rational else h
            ratio = rational[min(g.component, len(r)-1)]
            sr = max(1, int(round(n/tr.duration)))
            if m == "compound":
                geom = hg.lissajous_compound(h, n_points=n, n_periods=max(1, int(tr.duration)))
            elif m in ("lateral", "rotary", "trace_3d"):
                fn = {"lateral": hg.harmonograph_lateral, "rotary": hg.harmonograph_rotary, "trace_3d": hg.harmonograph_3d}[m]
                kw = {"rotation_freq": tr.rotation} if m == "rotary" else {}
                geom = fn(h, duration=tr.duration, sr=sr, **kw)
            elif m == "closed_2d":
                self._sampling(max(ratio.numerator, ratio.denominator), n)
                geom = hg.lissajous_2d(ratio, phase=tr.phase, n_points=n)
            elif m == "closed_3d":
                if len(r) < 3:
                    raise ValueError("closed_3d needs at least three components; trace_3d also supports smaller chords")
                period = math.lcm(*(v.denominator for v in rational[:3]))
                self._sampling(max(float(v)*period for v in rational[:3]), n)
                geom = hg.lissajous_3d(rational[:3], phases=ph[:3], amps=a[:3], n_points=n)
            elif m == "pairwise":
                if len(r) > 6:
                    raise ValueError("pairwise supports up to six components; reduce the tuning first")
                # Each cell closes independently. The bounded rational input limits the work.
                grid = hg.lissajous_pairwise_grid(rh, n_points=n, phase=tr.phase)
                count = len(grid)
                parts = [cell.coordinates * 0.8 + [2.2*j, -2.2*i] for i, row in enumerate(grid) for j, cell in enumerate(row)]
                geom = hg.GeometryData("curve_set_2d", parts, metadata={"kind": "pairwise", "rows": count})
            elif m in ("rose", "epicycloid", "hypocycloid"):
                if m != "rose" and float(ratio) <= 1:
                    raise ValueError("Rolling-circle curves need a selected ratio greater than 1")
                self._sampling(max(ratio.numerator, ratio.denominator), n)
                geom = {"rose": hg.rose_curve, "epicycloid": hg.epicycloid, "hypocycloid": hg.hypocycloid}[m](ratio, n_points=n)
            elif m == "tuning_circle":
                geom = hg.tuning_circle(h)
            elif m == "times_table":
                geom = hg.times_table_from_input(h, n_points=min(n, 512), mode=st.circle_mode)
            elif m == "interval_graph":
                geom = hg.interval_vector_diagram(h)
            elif m == "chord_graph":
                geom = hg.polygon_chord_pattern(h)
            elif m == "consonance_polygon":
                geom = hg.consonance_polygon(h)
            elif m == "stern_brocot":
                geom = hg.stern_brocot_tree(rh, max_depth=st.depth, layout="hyperbolic")
            elif m == "continued_fraction":
                geom = hg.continued_fraction_rectangles(ratio, depth=st.depth)
            elif m == "farey":
                geom = hg.farey_sequence_layout(st.order, layout="ford")
            elif m == "subharmonic_tree":
                if len(r)*sum(3**i for i in range(st.depth+1)) > 12000:
                    raise ValueError("Subharmonic tree exceeds 12,000 vertices; reduce depth or component count")
                geom = hg.subharmonic_tree(rh, depth=st.depth, n_harmonics=3, layout="polar")
            elif m == "ifs":
                geom = hg.ifs_harmonic(h, n_points=n, contraction=st.contraction, rng=np.random.default_rng(st.seed))
            elif m == "lsystem":
                if st.depth > 4:
                    raise ValueError("L-system depth is limited to 4")
                geom = hg.lsystem_from_ratios(rh, depth=st.depth)
            elif m == "recursive_polygon":
                if st.order * 8**st.depth > 50000:
                    raise ValueError("Recursive polygon exceeds its work budget; reduce depth or order")
                geom = hg.recursive_polygon(rh, depth=st.depth, n_sides=st.order)
            elif m == "pitch_lattice":
                geom = hg.self_similar_tuning(h, n_levels=st.depth, equave=h.equave)
            elif m == "tube":
                if len(r) < 3:
                    raise ValueError("A Lissajous tube needs at least three components")
                geom = hg.lissajous_tube(h, n_points=n, n_periods=max(1, int(tr.duration)), tube_radius=sf.tube_radius, n_sides=sf.tube_sides)
            elif m == "knot":
                self._sampling(max(v.numerator for v in rational), n)
                geom = hg.harmonic_knot(rh, n_points=n, tube_radius=sf.tube_radius, n_sides=sf.tube_sides)
            elif m in ("torus", "sphere", "cylinder"):
                geom = hg.harmonic_surface(rh, mode=m, resolution=min(res, 64))
            elif m == "point_cloud":
                geom = hg.harmonic_point_cloud(h, n_points=n, surface="sphere")
            elif m == "tree_3d":
                if st.depth > 3:
                    raise ValueError("3D tree depth is limited to 3")
                geom = hg.lsystem_3d(rh, depth=st.depth)
            elif m == "polyhedron":
                if st.depth > 3:
                    raise ValueError("Polyhedron depth is limited to 3")
                geom = hg.recursive_polyhedron(rh, depth=st.depth, solid=sf.solid)
            elif m in ("plate", "pairwise_plate", "symmetric_plate", "triple_plate", "circular_plate"):
                schemes = {"pairwise_plate": "pairwise_antisymmetric", "symmetric_plate": "pairwise_symmetric", "triple_plate": "triple_antisymmetric"}
                domain = hg.Circular() if m == "circular_plate" else hg.Rectangular()
                geom = hg.RigidPlate(domain=domain, mode_scheme=schemes.get(m, "per_ratio"),
                    mode_strategy=f.strategy, max_mode=f.max_mode, symmetry=f.symmetry, resolution=res).respond(rh)
            elif m == "elastic":
                geom = hg.Elastic(anisotropy_ratio=f.anisotropy, anisotropy_axis=f.axis, n_modes=min(24, f.max_mode*2), resolution=res).respond(h)
            elif m in ("harmonic_field", "quasicrystal", "wave_lattice"):
                fn = {"harmonic_field": hg.harmonic_interference_field_2d, "quasicrystal": hg.quasicrystal_field_2d, "wave_lattice": hg.standing_wave_lattice_2d}[m]
                kw = {"n_directions": st.order} if m == "harmonic_field" else {"n_fold": st.order} if m == "quasicrystal" else {}
                geom = fn(h, extent=f.extent, resolution=res, base_period=f.period, output=f.output, **kw)
            elif m == "vortex":
                geom = hg.vortex_field_2d(rh, extent=f.extent, resolution=res, output=f.output)
            elif m == "sources":
                geom = hg.interference_field_2d(h, extent=f.extent, resolution=res, base_wavelength=f.period, output=f.output, layout="circle")
            elif m == "acoustic":
                geom = hg.Acoustic(n_sources=st.order, extent=f.extent, resolution=res, base_frequency=1/f.period).respond(h)
            elif m == "faraday":
                geom = hg.Faraday(pattern=f.faraday_pattern, base_wavenumber=2*np.pi/f.period, extent=f.extent,
                                  resolution=res, output=f.output, viscosity=f.viscosity, seed=st.seed).respond(h)
            elif m == "spherical_field":
                geom = hg.spherical_harmonic_from_input(rh, mode_rule="sectoral", max_l=f.max_mode, n_theta=res, n_phi=res)
            elif m == "spherical_mesh":
                geom = hg.spherical_harmonic_mesh(rh, mode_rule="sectoral", max_l=f.max_mode, n_theta=min(res, 64), n_phi=min(res, 64))
            else:
                raise ValueError(f"Unknown geometry method {m}")
        geom.metadata["method"] = m
        geom.metadata["morph"] = float(input.meta.get("morph", 0.0))
        frame = {"type": geom.geom_type, "coordinates": geom.coordinates,
                 "info": json.dumps({"parameters": geom.parameters, "metadata": geom.metadata}, default=json_value)}
        for key in ("edges", "faces", "weights"):
            value = getattr(geom, key)
            if value is not None:
                frame[key] = value
        if geom.field_grid is not None:
            frame["grid"] = geom.field_grid
        return {"geometry": encode(frame)}

    @staticmethod
    def _sampling(cycles, points):
        if cycles * 8 > points:
            raise ValueError("Closed geometry needs at least eight points per winding; increase points, lower denominator, or use trace_3d")
