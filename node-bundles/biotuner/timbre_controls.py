"""TimbreControls — a tuning as continuous synth controls, at frame rate.

The other half of `VitalPreset`. A preset is written once and carries structure a stream cannot —
wavetables, modulation routings, the coupling analysis that costs a second to compute. This node
carries what a frame CAN: the timbre's shape, every frame, as numbers a plugin parameter binds to.

Takes a TUNING (ratios inside an octave), as `Tuning` emits them. The calculations
use pairwise similarity and do not run an iterative timbre optimizer.

It takes NO amplitudes, and that is a decision rather than an omission. `compute_peak_ratios`
answers every PAIRWISE ratio, deduplicated and folded — five peaks give ten degrees — so degree
`k` stands in no relation to peak `k` and an amplitude cannot be aligned to it. The information
needed to align them does not survive the tuning, so it cannot be repaired here: an amplitude
belongs to a peak, and weighting it belongs where the peaks still exist. `tilt` shapes the
partials instead.


The analysis outputs preserve leading axes. The audio outputs select one tuning
row and use fixed [voices, 1] columns without NaN values. Pass each through an
audio:SignalIn in direct mode. Use pitch for Osc.pitch and gain for Gain.gain,
or connect voices to a VST instrument's voice input. This plays partials as
separate notes; a VitalPreset instead puts the spectrum inside an oscillator.

VST notes take pitch and velocity on the gate rise. Release and retrigger after
changing the tuning; native Osc can follow pitch continuously. A receiving
VST must support the host's per-channel pitch bend for exact microtonal notes.
"""

from fractions import Fraction

import numpy as np
from biotuner.metrics import dyad_similarity
import goofi

C4_HZ = 261.63


class TimbreControls(goofi.Node):
    """A tuning as continuous, bindable synth controls.

    Inputs:
      input  a tuning: ratios inside an octave, as `Tuning` emits them
      gate   optional scalar or column of gates for the selected partials.
             Positive values sound a voice; missing input sustains all voices.

    Outputs:
      partials      the ratios as frequencies over `base_freq`, in Hz
      amplitudes    per partial, 0 to 1, normalized so the loudest is 1
      weights       per partial, how consonant it is against the rest, 0 to 1
      brightness    the amplitude-weighted centroid, 0 to 1 across the partial span — the one to bind
                    to a filter cutoff
      spread        how far the degrees sit from simple just ratios, in cents against `spread_span`,
                    0 to 1: a just scale is 0, a tempered or irrational one higher. Binds to
                    detune, unison or an inharmonic control
      harmonicity   the mean consonance of the whole set, 0 to 1
      pitch         selected partials in volts per octave, zero at C4; [voices, 1]
      gain          selected amplitudes divided by their sum, then gated; [voices, 1]
      gate          selected voice gates, with unused voices off; [voices, 1]
      voices        pitches followed by gated velocities; [2 * voices, 1], for VST voice

    Use SignalIn in direct mode for pitch, gain, gate, and voices. A native
    patch is Osc -> Gain -> Mixdown -> AudioOut, with pitch and gain references.
    The VST voice cable plays the partials as notes, not as one custom wavetable.
    Select one analysis row before binding brightness, spread, or harmonicity
    to a plugin parameter. Those controls are normalized to 0..1; native filter
    pitch and other physical units need an explicit range mapping.
    """

    TAGS = ["transform"]
    INPUTS = {
        "input": goofi.InputSlot(goofi.DataType.ARRAY, required=True),
        "gate": goofi.InputSlot(goofi.DataType.ARRAY, required=False),
    }
    OUTPUTS = {
        "partials": goofi.DataType.ARRAY,
        "amplitudes": goofi.DataType.ARRAY,
        "weights": goofi.DataType.ARRAY,
        "brightness": goofi.DataType.ARRAY,
        "spread": goofi.DataType.ARRAY,
        "harmonicity": goofi.DataType.ARRAY,
        "pitch": goofi.DataType.ARRAY,
        "gain": goofi.DataType.ARRAY,
        "gate": goofi.DataType.ARRAY,
        "voices": goofi.DataType.ARRAY,
    }
    PARAMS = {
        "timbre": {
            "base_freq": goofi.FloatParam(220.0, 20.0, 2000.0, doc="The frequency ratio 1 sits at, in Hz."),
            "tilt": goofi.FloatParam(0.0, -2.0, 2.0, doc="Amplitude rolloff per partial: above 0 favours the low ones."),
            "spread_span": goofi.FloatParam(25.0, 1.0, 200.0, doc="Cents away from just that reads as spread 1."),
            "justLimit": goofi.IntParam(8, 2, 32, doc="Largest denominator a degree may be called just by."),
        },
        "voice": {
            "row": goofi.IntParam(0, 0, 65535, doc="Tuning row used by the audio outputs, counting from zero."),
            "voices": goofi.IntParam(8, 1, 8, doc="Fixed voice count. Use the lowest partials; unused voices are silent."),
            "velocity": goofi.FloatParam(0.7, 0.0, 1.0, doc="VST note velocity, scaled by each partial's amplitude."),
        },
    }

    @staticmethod
    def _cents_from_just(ratios, limit):
        """Mean distance, in cents, from each degree to the simplest just ratio near it."""
        away = []
        for r in ratios:
            near = Fraction(float(r)).limit_denominator(int(limit))
            away.append(abs(1200.0 * np.log2(float(r) / float(near))) if float(near) > 0 else 0.0)
        return float(np.mean(away)) if away else 0.0

    def process(self, input, gate=None):
        p = self.params.timbre
        x = np.asarray(input.data, dtype=np.float64)
        if x.ndim == 0:
            raise ValueError("TimbreControls reads a scale, not a single number")

        lead = x.shape[:-1]
        rows = x.reshape(int(np.prod(lead)) if lead else 1, x.shape[-1])
        if self.params.voice.row >= len(rows):
            raise ValueError("TimbreControls voice.row is outside the input's tuning rows")
        scales = [np.unique(r[np.isfinite(r) & (r > 0)]) for r in rows]
        width = max((len(r) for r in scales), default=0) or 1
        part = np.full((rows.shape[0], width), np.nan)
        amp = np.full((rows.shape[0], width), np.nan)
        wgt = np.full((rows.shape[0], width), np.nan)
        scal = np.full((rows.shape[0], 3), np.nan)

        for i, ratios in enumerate(scales):
            if ratios.size == 0:
                continue
            freqs = ratios * p.base_freq

            # `tilt` alone shapes the amplitudes, and the loudest partial is 1.
            log_a = -p.tilt * np.log(ratios)
            a = np.exp(log_a - log_a.max())

            # How consonant each partial is against every other, as the mean of its pairs.
            w = np.full(1, 100.0) if ratios.size == 1 else np.array(
                [np.mean([dyad_similarity(f / g) for j, g in enumerate(ratios) if j != k]) for k, f in enumerate(ratios)]
            )
            w = np.clip(w / 100.0, 0.0, 1.0)

            centroid = float(np.sum(freqs * a) / np.sum(a)) if a.sum() > 0 else float(freqs[0])
            span = float(freqs[-1] - freqs[0])
            brightness = (centroid - freqs[0]) / span if span > 0 else 0.0
            # Distance from the nearest SIMPLE JUST ratio. Measuring against the nearest whole
            # number cannot work on a scale: every ratio is folded into `[1, 2)`, so the distance
            # is to 1 or to 2, which an evenly spread scale maximises by construction — it rated a
            # just major scale as more bell-like than a deliberately inharmonic one.
            spread = self._cents_from_just(ratios, p.justLimit) / p.spread_span

            part[i, : ratios.size] = freqs
            amp[i, : a.size] = a
            wgt[i, : w.size] = w
            scal[i] = (np.clip(brightness, 0.0, 1.0), np.clip(spread, 0.0, 1.0), float(np.mean(w)))

        f32 = lambda v, shape: v.reshape(shape).astype(np.float32)
        s = scal.reshape(lead + (3,)).astype(np.float32)
        meta = input.drop_axis(-1)
        # The frequency frame owns its reference frequency, including when exported.
        partial_meta = {**meta, "base_freq": p.base_freq}
        voice = self.params.voice
        n = min(voice.voices, len(scales[voice.row]))
        pitch = np.zeros(voice.voices)
        amplitude = np.zeros(voice.voices)
        pitch[:n] = np.log2(part[voice.row, :n] / C4_HZ)
        amplitude[:n] = amp[voice.row, :n]
        gates = np.ones(voice.voices)
        if gate is not None:
            g = np.asarray(gate.data, dtype=np.float64)
            if g.ndim > 2 or (g.ndim == 2 and g.shape[1] != 1):
                raise ValueError("TimbreControls gate needs a scalar or a [voices, 1] column")
            g = g.ravel()
            if g.size == 1:
                gates[:] = np.isfinite(g[0]) and g[0] > 0
            elif g.size in (n, voice.voices):
                gates[:] = 0
                gates[:g.size] = np.isfinite(g) & (g > 0)
            else:
                raise ValueError("TimbreControls gate count must match the selected partials or voice count")
        gates[n:] = 0
        gain = amplitude / amplitude.sum() if amplitude.sum() > 0 else amplitude
        gain = gain * gates
        velocity = amplitude * voice.velocity * gates
        # VST note numbers stop at 0 and 127. Do not silently transpose an out-of-range partial.
        velocity[(pitch < -5) | (pitch > 67 / 12)] = 0
        packed = np.concatenate([np.clip(pitch, -5, 67 / 12), velocity])
        voice_meta = {"channels": {"dim0": [f"partial{i + 1}" for i in range(voice.voices)]}}
        packed_meta = {"channels": {"dim0":
            [f"pitch{i + 1}" for i in range(voice.voices)]
            + [f"velocity{i + 1}" for i in range(voice.voices)]}}
        column = lambda v: v.astype(np.float32).reshape(-1, 1)
        return {
            "partials": (f32(part, lead + (width,)), partial_meta),
            "amplitudes": (f32(amp, lead + (width,)), meta),
            "weights": (f32(wgt, lead + (width,)), meta),
            "brightness": (s[..., 0], meta),
            "spread": (s[..., 1], meta),
            "harmonicity": (s[..., 2], meta),
            "pitch": (column(pitch), voice_meta),
            "gain": (column(gain), voice_meta),
            "gate": (column(gates), voice_meta),
            "voices": (column(packed), packed_meta),
        }
