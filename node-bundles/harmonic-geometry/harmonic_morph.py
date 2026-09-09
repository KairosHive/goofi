"""HarmonicMorph: two peak structures or tunings as one continuous harmonic frame.

Wire Tuning.tuning to a/b in ratios mode, or Peaks.peaks/HarmonicSpectrum.peaks
in peaks mode. Wire amplitudes only when they belong to those exact components.
NaN padding is removed with its amplitude and phase. Select one input row.
Unwired inputs use the ratio lists; a wired empty input means silence.

Sorted rank pairs components. Extra components keep their frequency and fade.
This is not peak tracking. The harmonic TABLE keeps zero-amplitude components
for stable geometry; tuning omits silent components for the existing bundle.
packed is [4, 32]: ratios, amplitudes, phases, damping, zero-padded for shaders.
"""

from fractions import Fraction
import numpy as np
from biotuner.harmonic_input import HarmonicInput
from biotuner.harmonic_geometry import (
    interpolate_chords, fade_in_components, extend_harmonics, extend_subharmonics,
)
import goofi


def row_values(data, row, name):
    x = np.asarray(data.data, dtype=np.float64)
    if x.ndim == 0:
        raise ValueError(f"{name} needs a component axis")
    rows = x.reshape(int(np.prod(x.shape[:-1])) if x.ndim > 1 else 1, x.shape[-1])
    if row >= len(rows):
        raise ValueError(f"{name} row {row} is outside {len(rows)} rows")
    return rows[row]


def unit_amplitudes(a):
    return a / a.sum() if a.sum() > 0 else a


def merge_components(r, a, ph):
    """Combine repeated frequencies before a component fade."""
    # The wire is float32. Two computations of the same tone can differ in float64.
    unique, inverse = np.unique(np.asarray(r, dtype=np.float32).astype(np.float64), return_inverse=True)
    weights = np.bincount(inverse, weights=a, minlength=len(unique))
    phase = np.angle(np.bincount(inverse, weights=a * np.cos(ph), minlength=len(unique))
                     + 1j * np.bincount(inverse, weights=a * np.sin(ph), minlength=len(unique)))
    return unique, weights, phase


class HarmonicMorph(goofi.Node):
    """Morph tunings, phases, and component counts; share the result across media."""

    TAGS = ["generator", "transform"]
    INPUTS = {k: goofi.InputSlot(goofi.DataType.ARRAY, required=False)
              for k in ("a", "b", "ampsA", "ampsB", "phasesA", "phasesB", "mix")}
    OUTPUTS = {"harmonic": goofi.DataType.TABLE, **{k: goofi.DataType.ARRAY
               for k in ("tuning", "peaks", "amps", "phases", "packed", "mix")}}
    PARAMS = {
        "source": {
            "ratios_a": goofi.StringParam("1, 5/4, 3/2", doc="Unwired A: positive ratios, as decimals or fractions."),
            "ratios_b": goofi.StringParam("1, 6/5, 3/2, 7/4", doc="Unwired B: positive ratios, as decimals or fractions."),
            "mode_a": goofi.StringParam("ratios", ["ratios", "peaks"], doc="Wired A contains ratios or frequencies in Hz."),
            "mode_b": goofi.StringParam("ratios", ["ratios", "peaks"], doc="Wired B contains ratios or frequencies in Hz."),
            "row": goofi.IntParam(0, 0, 65535, doc="Flattened leading row to select from each input."),
            "amps_scale": goofi.StringParam("linear", ["linear", "db_power", "db_amplitude"], doc="Scale of both amplitude inputs; dB is converted before normalization."),
            "base_freq": goofi.FloatParam(110.0, 0.1, 2000.0, doc="Hz for ratio 1 at the peaks output; geometry uses relative frequencies."),
            "equave": goofi.FloatParam(2.0, 1.01, 8.0, doc="Pitch-circle period; 2 is an octave, 3 is a tritave."),
        },
        "morph": {
            "mix": goofi.FloatParam(0.0, 0.0, 1.0, doc="0 is A, 1 is B. A wired mix overrides this value."),
            "space": goofi.StringParam("pitch", ["pitch", "ratio"], doc="Pitch interpolates log frequency; ratio interpolates frequency ratios."),
            "easing": goofi.StringParam("smooth", ["linear", "smooth"], doc="Smooth gives zero slope at each endpoint."),
            "phase": goofi.FloatParam(0.0, -1000.0, 1000.0, doc="Shared phase offset in radians; bind an LFO for motion."),
            "phase_spread": goofi.FloatParam(0.7, -12.57, 12.57, doc="Additional phase per sorted component, in radians."),
            "stretch": goofi.FloatParam(1.0, 0.1, 4.0, doc="Raise ratios to this power to stretch intervals continuously."),
            "damping": goofi.FloatParam(0.025, 0.0, 2.0, doc="Decay per second for harmonograph traces."),
        },
        "extension": {
            "kind": goofi.StringParam("none", ["none", "harmonics", "subharmonics"], doc="Grow multiples or divisions of each component."),
            "count": goofi.IntParam(2, 1, 7, doc="Extra levels: count 2 adds the second and third harmonics."),
            "amount": goofi.FloatParam(0.0, 0.0, 1.0, doc="Fade new components from zero to their full weight."),
            "decay": goofi.FloatParam(1.0, 0.0, 4.0, doc="Amplitude falls as harmonic number to this power."),
        },
        "common": {"autotrigger": goofi.BoolParam(True), "max_frequency": goofi.FloatParam(20.0, 0.0, 60.0)},
    }

    def _source(self, data, amp, phase, side):
        p = self.params.source
        if data is None:
            text = getattr(p, f"ratios_{side}")
            try:
                r = np.array([float(Fraction(s.strip())) for s in text.split(",") if s.strip()])
            except (ValueError, ZeroDivisionError) as e:
                raise ValueError(f"ratios_{side} must contain positive decimals or fractions") from e
        else:
            r = row_values(data, p.row, side)
        if np.any(np.isinf(r)) or np.any(r[np.isfinite(r)] <= 0):
            raise ValueError(f"{side} components must be positive and finite; only NaN padding is allowed")
        valid = ~np.isnan(r)
        if len(r) > 4096:
            raise ValueError("Select a peak or tuning list, not a waveform (maximum input width 4096)")

        def aligned(slot, default, name):
            if slot is None:
                return np.full(valid.sum(), default)
            values = row_values(slot, p.row if np.asarray(slot.data).ndim > 1 else 0, name)
            if values.shape != r.shape:
                raise ValueError(f"{name} must have the same component count as {side}")
            values = values[valid]
            if not np.all(np.isfinite(values)):
                raise ValueError(f"{name} must be finite at every valid component")
            return values

        a = aligned(amp, 1.0, f"amps_{side}")
        ph = aligned(phase, 0.0, f"phases_{side}")
        if amp is not None and p.amps_scale != "linear" and a.size:
            a = 10.0 ** ((a - a.max()) / (10 if p.amps_scale == "db_power" else 20))
        if np.any(a < 0):
            raise ValueError("Linear amplitudes cannot be negative; select the correct amps_scale")
        r = r[valid]
        if data is not None and getattr(p, f"mode_{side}") == "peaks" and r.size:
            r = r / r.min()
        r, a, ph = merge_components(r, a, ph)
        if r.size > 32:
            raise ValueError("HarmonicMorph supports at most 32 components; reduce the tuning first")
        return r, unit_amplitudes(a), ph

    def process(self, a=None, b=None, ampsA=None, ampsB=None, phasesA=None, phasesB=None, mix=None):
        p, source = self.params.morph, self.params.source
        t = float(p.mix)
        if mix is not None:
            value = np.asarray(mix.data).ravel()
            if value.size != 1 or not np.isfinite(value[0]):
                raise ValueError("mix needs one finite value")
            t = float(np.clip(value[0], 0.0, 1.0))
        if p.easing == "smooth":
            t = t * t * (3.0 - 2.0 * t)
        ra, aa, pa = self._source(a, ampsA, phasesA, "a")
        rb, ab, pb = self._source(b, ampsB, phasesB, "b")
        chord_a = HarmonicInput(ratios=ra.tolist(), amplitudes=aa.tolist(), phases=pa.tolist())
        chord_b = HarmonicInput(ratios=rb.tolist(), amplitudes=ab.tolist(), phases=pb.tolist())
        chord = interpolate_chords(chord_a, chord_b, t)
        r = np.asarray([float(v) for v in chord.to_ratios()])
        n = min(len(ra), len(rb))
        # Keep exact floating interpolation on the wire; Biotuner rationalizes its descriptor.
        r[:n] = np.exp((1-t)*np.log(ra[:n]) + t*np.log(rb[:n])) if p.space == "pitch" else (1-t)*ra[:n] + t*rb[:n]
        weights = np.r_[(1-t)*aa[:n] + t*ab[:n], (1-t)*aa[n:], t*ab[n:]]
        phases = np.r_[pa[:n] + t*np.angle(np.exp(1j*(pb[:n]-pa[:n]))), pa[n:], pb[n:]]
        r = np.power(r, p.stretch)
        phases += p.phase + p.phase_spread * np.arange(len(r))
        ext = self.params.extension
        if ext.kind != "none" and r.size and weights.sum() > 0:
            r, weights, phases = merge_components(r, weights, phases)
            base = HarmonicInput(ratios=[Fraction(float(v)) for v in r], amplitudes=weights.tolist(), phases=phases.tolist())
            fn = extend_harmonics if ext.kind == "harmonics" else extend_subharmonics
            expanded = fn(base, n_harmonics=ext.count, decay=ext.decay)
            er, ea, ep = merge_components(expanded.to_peaks(), np.asarray(expanded.amplitudes), np.asarray(expanded.phases))
            expanded = HarmonicInput(ratios=[Fraction(float(v)) for v in er], amplitudes=ea.tolist(), phases=ep.tolist())
            # Both sets already use wire precision. A tolerance can activate a nearby new tone.
            grown = fade_in_components(base, expanded, ext.amount, match_tol=0.0)
            base_phases = dict(zip(r, phases))
            phases = ep.copy()
            for i, value in enumerate(er):
                if value in base_phases:
                    start = base_phases[value]
                    phases[i] = start + ext.amount*np.angle(np.exp(1j*(ep[i]-start)))
            r = er
            weights = np.asarray(grown.amplitudes) * weights.sum()
        if len(r) > 32:
            raise ValueError("Extension exceeds 32 components; reduce source count or extension/count")
        if not np.all(np.isfinite(r)) or np.any(r > 1e6):
            raise ValueError("Stretched ratios exceed the supported range (1,000,000)")
        order = np.argsort(r, kind="stable")
        r, weights, phases = r[order], weights[order], phases[order]
        damping = np.full(len(r), p.damping)
        meta = {"base_freq": source.base_freq, "equave": source.equave, "morph": t}
        harmonic = {k: v.astype(np.float32) for k, v in
                    zip(("ratios", "amplitudes", "phases", "damping"), (r, weights, phases, damping))}
        packed = np.zeros((4, 32), dtype=np.float32)
        packed[:, :len(r)] = np.stack((r, weights, phases, damping))
        active = weights > 0
        return {
            "harmonic": (harmonic, meta),
            "tuning": (r[active].astype(np.float32), {**meta, "units": "ratio"}),
            "peaks": ((r[active]*source.base_freq).astype(np.float32), {**meta, "units": "Hz"}),
            "amps": (weights[active].astype(np.float32), meta),
            "phases": (phases[active].astype(np.float32), meta),
            "packed": (packed, {**meta, "channels": {"dim0": ["ratio", "amplitude", "phase", "damping"]}}),
            "mix": (np.float32(t), {}),
        }
