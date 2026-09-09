"""HarmonicModes: bounded Biotuner Chladni modes for shader synthesis.

input and target are harmonic TABLEs. Each is mapped independently by Biotuner.
Then mode coordinates, amplitudes, and phases interpolate by component rank.
Alternatively, transition takes one TABLE with input and target harmonic TABLEs
and a scalar mix. RatioSequence supplies this complete frame so endpoint changes
and the mix reset cannot arrive on separate clocks at a step boundary.
Fractional intermediate modes draw a continuous visual field; they are not
eigenmodes of the original closed plate. Select fields interpolation to retain
fixed endpoint frequencies instead.

modes is [4,32], or [4,64] for two fixed fields: m, n, amplitude, phase.
Unused columns have zero amplitude. Chord pairs use Biotuner's common-denominator
integer chord and pair selection, with uniform pair weights and zero phases.
The mapping output reports both ratios and actual pairs, including any mode cap.
"""

import numpy as np
from biotuner.harmonic_geometry.media.eigenmode.rigid_plate import ratios_to_modes, chord_to_int_modes, chladni_field_pairwise
import goofi


class HarmonicModes(goofi.Node):
    """Map harmonic ratios to Chladni modes and interpolate two mode structures.

    Use separate input/target harmonic frames and mix, or a single transition
    TABLE carrying those three fields. RatioSequence.transition supplies the
    latter so a new step's endpoints and its mix reset remain synchronized.
    Endpoints map to integer modes first; intermediate coordinates are continuous
    visual fields. Connecting both input routes reports an error.
    Fields interpolation keeps endpoint wavenumbers fixed and only blends their
    weights; this removes the corner-anchored expansion of coordinate motion.
    """

    TAGS = ["transform"]
    INPUTS = {"input": goofi.InputSlot(goofi.DataType.TABLE, required=False),
              "target": goofi.InputSlot(goofi.DataType.TABLE, required=False),
              "transition": goofi.InputSlot(goofi.DataType.TABLE, required=False),
              "mix": goofi.InputSlot(goofi.DataType.ARRAY, required=False)}
    OUTPUTS = {"modes": goofi.DataType.ARRAY, "mapping": goofi.DataType.STRING}
    PARAMS = {"modes": {
        "mapping": goofi.StringParam("per ratio", ["per ratio", "chord pairs"], doc="Per-ratio rational pairs, or Biotuner's common-denominator chord with pairwise cosine products. Chord pairs use uniform pair weights and zero phases."),
        "interpolation": goofi.StringParam("coordinates", ["coordinates", "fields"], doc="Coordinates change wavenumbers about the grid origin. Fields blend fixed endpoint modes without spatial scaling; recommended for Chladni state walks."),
        "pairs": goofi.StringParam("auto", ["auto", "all", "root", "adjacent"], doc="Biotuner pair subset for chord pairs. Auto uses all pairs for up to three ratios, root pairs for larger chords."),
        "strategy": goofi.StringParam("best_simple", ["stern_brocot", "continued_fraction", "rounded", "best_simple"], doc="Biotuner's ratio-to-mode mapping."),
        "max_mode": goofi.IntParam(12, 1, 24, doc="Largest mode index on either axis."),
        "mix": goofi.FloatParam(0.0, 0.0, 1.0, doc="Interpolation from input modes to target modes; wire mix or set here."),
    }}

    def _modes(self, data):
        table, p = data.table, self.params.modes
        try:
            r, a, ph = [np.asarray(table[k].data, dtype=np.float64) for k in ("ratios", "amplitudes", "phases")]
        except KeyError as e:
            raise ValueError("HarmonicModes needs HarmonicMorph.harmonic") from e
        if r.ndim != 1 or a.shape != r.shape or ph.shape != r.shape or len(r) > 32:
            raise ValueError("Mode input needs aligned vectors of at most 32 components")
        if not np.all(np.isfinite(np.r_[r, a, ph])) or np.any(r <= 0) or np.any(a < 0):
            raise ValueError("Mode components must be finite, with positive ratios and nonnegative amplitudes")
        description = ", ".join(f"{value:.6g}" for value in r)
        if p.mapping == "chord pairs":
            active = list(dict.fromkeys(r[a > 0]))
            if len(active) < 2:
                raise ValueError("Chord pairs need at least two distinct active ratios")
            integers = chord_to_int_modes(active)
            # Read the public builder's chosen pairs and weights, including its cap and subset rules.
            field = chladni_field_pairwise(integers, resolution=4, max_mode=p.max_mode, pair_subset=p.pairs)
            pairs = np.asarray(field.parameters['pairs'], dtype=np.float64)
            if len(pairs) > 32:
                raise ValueError("More than 32 chord pairs; use root or adjacent pairs, or fewer ratios")
            a = np.asarray(field.parameters['amps'])
            ph = np.asarray(field.parameters['phases'])
            description += " | integer chord " + str(integers)
            if max(integers) > p.max_mode:
                description += f" | scaled by {p.max_mode/max(integers):.6g} to fit max_mode"
        else:
            pairs = np.asarray(ratios_to_modes(r, strategy=p.strategy, max_mode=p.max_mode)).reshape(-1, 2)
        count = len(pairs)
        modes = np.zeros((4, 32), dtype=np.float64)
        if count:
            modes[:2, :count] = pairs.T
            modes[2, :count] = a
            modes[3, :count] = ph
        description += " | pairs " + ", ".join(f"({m:g}, {n:g})" for m, n in pairs)
        return modes, count, description

    def process(self, input=None, target=None, mix=None, transition=None):
        if transition is not None:
            if any(value is not None for value in (input, target, mix)):
                raise ValueError("Connect either transition or separate input, target, and mix")
            try:
                input, target, mix = [transition.table[k] for k in ("input", "target", "mix")]
            except KeyError as e:
                raise ValueError("A transition needs input, target, and mix") from e
        if input is None:
            return None
        modes, na, source_text = self._modes(input)
        target_text = source_text
        t = self.params.modes.mix
        if mix is not None:
            v = np.asarray(mix.data).ravel()
            if len(v) != 1 or not np.isfinite(v[0]):
                raise ValueError("HarmonicModes mix needs one finite value")
            t = float(np.clip(v[0], 0, 1))
        if target is not None:
            other, nb, target_text = self._modes(target)
            if self.params.modes.interpolation == 'fields':
                modes[2] *= 1-t
                other[2] *= t
                modes = np.concatenate((modes, other), axis=1)
            else:
                n = min(na, nb)
                modes[:2, :n] = (1-t)*modes[:2, :n]+t*other[:2, :n]
                modes[3, :n] += t*np.angle(np.exp(1j*(other[3, :n]-modes[3, :n])))
                modes[[0, 1, 3], na:nb] = other[[0, 1, 3], na:nb]
                modes[2] = (1-t)*modes[2]+t*other[2]
        return {'modes': (modes.astype(np.float32), {"channels": {"dim0": ["m", "n", "amplitude", "phase"]}}),
                'mapping': f"A: {source_text}\nB: {target_text}\n{self.params.modes.interpolation} | mix {t:.4f}"}
