"""HarmonicVoices: the shared harmonic frame as continuous native synth controls.

Unlike TimbreControls, this adapter keeps the frame's measured amplitudes and
silent component slots. Pitch is volts per octave, zero at C4 (261.63 Hz).
Pass pitch and gain through audio:SignalIn in direct mode, then reference them
from Osc.osc.pitch and Gain.gain.gain. Mixdown combines the fixed voice column.
No audio device is opened by this node. The native oscillator follows pitch
continuously; a VST note protocol does not guarantee that behavior.
"""

import numpy as np
import goofi


class HarmonicVoices(goofi.Node):
    """Keep component fades when one harmonic frame drives sound and geometry."""

    TAGS = ["transform", "music"]
    INPUTS = {"input": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {k: goofi.DataType.ARRAY for k in ("pitch", "gain")}
    PARAMS = {"sound": {
        "voices": goofi.IntParam(8, 1, 8, doc="Fixed voice count; takes the lowest components, including fading slots."),
        "level": goofi.FloatParam(0.2, 0.0, 1.0, doc="Total gain ceiling. Zero mutes the sound without changing the geometry."),
        "transpose": goofi.FloatParam(0.0, -4.0, 4.0, doc="Pitch shift in octaves, relative to harmonic metadata base_freq."),
    }}

    def process(self, input):
        p = self.params.sound
        values = np.asarray(input.data, dtype=np.float64)
        if values.ndim != 2 or values.shape[0] != 4:
            raise ValueError("HarmonicVoices needs a [4,N] harmonic ARRAY")
        r, a = values[:2]
        base = float(input.meta.get("base_freq", 110.0))
        if r.ndim != 1 or r.shape != a.shape or len(r) > 32 or not np.all(np.isfinite(np.r_[r, a, base])) or base <= 0 or np.any(r <= 0) or np.any(a < 0):
            raise ValueError("Voices need aligned positive ratios and nonnegative amplitudes, with finite positive base_freq")
        count = min(p.voices, len(r))
        hz = np.zeros((p.voices, 1), dtype=np.float32)
        pitch, gain = np.zeros_like(hz), np.zeros_like(hz)
        hz[:count, 0] = base * r[:count] * 2**p.transpose
        if np.any(hz > 24000):
            raise ValueError("Voice frequency exceeds 24 kHz; lower base_freq or transpose")
        pitch[:count, 0] = np.log2(hz[:count, 0] / 261.63)
        gain[:count, 0] = a[:count] / max(1.0, a.sum()) * p.level
        return {"pitch": (pitch, {"units": "V/oct"}), "gain": (gain, {})}
