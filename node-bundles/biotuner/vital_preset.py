"""VitalPreset writes an oscillator preset from a tuning or explicit partials.

Connect either Tuning.tuning to input, or TimbreControls.partials and amplitudes
to the matching inputs. Explicit partials preserve the measured amplitude shape
and bypass tuning matching. Their base_freq metadata supplies the wavetable's
reference frequency. The preset/base_freq parameter is used if it is absent.

Each write pulse exports the selected row of the latest input frames. Files go
to the patch workspace by default. Load the .vital file in Vital's own browser;
it is not a VST3 component-state file and cannot be connected to a voice input.

spectral and bell use the same single-cycle wavetable with different envelopes
and effects. Fractional partial ratios shape that cycle, but periodic wavetable
playback cannot preserve exact inharmonic partial frequencies. Use the native
Osc path from TimbreControls when exact partial frequencies are required.
"""

import os

import numpy as np
from biotuner.harmonic_timbre import Timbre, timbre_from_ratios
from biotuner.harmonic_timbre.exporters import to_vital
import goofi


KINDS = {
    "spectral": to_vital.to_vital_spectral,
    "bell": to_vital.to_vital_inharmonic,
    "wavetableMorph": to_vital.to_vital_wavetable_morph,
    "ensemble": to_vital.to_vital_ensemble,
}


class VitalPreset(goofi.Node):
    """Export a tuning or explicit partials as a Vital oscillator preset.

    Inputs:
      input       tuning ratios; use this OR the partials/amplitudes pair
      partials    frequencies in Hz, as TimbreControls emits them
      amplitudes  linear amplitudes aligned with partials, not power in dB
      signal      optional raw biosignal for ensemble, with sfreq metadata

    Outputs:
      path   last successful preset path, or the ensemble directory
      files  paths of all presets, settings companions, and the ensemble manifest

    Select a tuning row with preset/row. Each write pulse uses the latest frames.
    Load the .vital file through Vital's preset browser. It supplies the sound;
    MIDI or a voice cable must still supply the notes. bell changes the envelope
    and effects; a periodic wavetable does not retain exact inharmonic pitches.
    """

    TAGS = ["output"]
    INPUTS = {
        "input": goofi.InputSlot(goofi.DataType.ARRAY, required=False),
        "partials": goofi.InputSlot(goofi.DataType.ARRAY, required=False),
        "amplitudes": goofi.InputSlot(goofi.DataType.ARRAY, required=False),
        "signal": goofi.InputSlot(goofi.DataType.ARRAY, required=False),
    }
    OUTPUTS = {"path": goofi.DataType.STRING, "files": goofi.DataType.TABLE}
    PARAMS = {
        "preset": {
            "kind": goofi.StringParam("spectral", list(KINDS), doc="spectral: pad; bell: percussive; wavetableMorph: moving spectrum; ensemble: several presets."),
            "name": goofi.StringParam("biotuner", doc="Preset name and file stem, without a path or extension."),
            "folder": goofi.StringParam("", doc="Export folder. Empty uses presets in the patch workspace."),
            "row": goofi.IntParam(0, 0, 65535, doc="Tuning or partial row to export, counting from zero."),
            "signalRow": goofi.IntParam(0, 0, 65535, doc="Raw signal row used by ensemble."),
            "base_freq": goofi.FloatParam(220.0, 20.0, 2000.0, doc="Tuning reference in Hz; explicit partials use their base_freq metadata when present."),
            "matching": goofi.StringParam(
                "consonance_weighted",
                ["consonance_weighted", "direct", "sethares", "harmonic_entropy", "hybrid"],
                doc="Tuning input only: how to fit partials. Optimizers run on the write pulse.",
            ),
            "write": goofi.PulseParam(doc="Export the selected row of the latest input frames."),
        }
    }

    def setup(self):
        self.frames = (None, None, None, None)
        self.written = {"path": "", "files": {}}

    def process(self, input=None, partials=None, amplitudes=None, signal=None):
        # Replace the complete snapshot so disconnected inputs cannot survive a write.
        self.frames = (input, partials, amplitudes, signal)
        return self.written

    @staticmethod
    def _row(frame, index, name):
        x = np.asarray(frame.data, dtype=np.float64)
        if x.ndim == 0:
            raise ValueError(f"VitalPreset {name} needs a list, not a scalar")
        count = int(np.prod(x.shape[:-1])) if x.ndim > 1 else 1
        if index >= count:
            raise ValueError(f"VitalPreset {name} row is outside the input")
        return x.reshape(count, x.shape[-1])[index]

    def pulse_preset_write(self):
        p = self.params.preset
        input, partials, amplitudes, signal = self.frames
        name = p.name.strip() or "biotuner"
        if name in (".", "..") or any(c in name for c in '<>:"/\\|?*') or name.endswith("."):
            raise ValueError("VitalPreset name must be a file stem without path separators")
        if input is not None and (partials is not None or amplitudes is not None):
            raise ValueError("VitalPreset accepts a tuning OR partials and amplitudes, not both")
        if partials is not None or amplitudes is not None:
            if partials is None or amplitudes is None:
                raise ValueError("VitalPreset needs both partials and amplitudes")
            if np.shape(partials.data) != np.shape(amplitudes.data):
                raise ValueError("VitalPreset partials and amplitudes must have the same shape")
            hz = self._row(partials, p.row, "partials")
            amps = self._row(amplitudes, p.row, "amplitudes")
            valid = np.isfinite(hz) & (hz > 0) & np.isfinite(amps)
            hz, amps = hz[valid], amps[valid]
            if not hz.size or np.any(amps < 0) or not np.any(amps > 0):
                raise ValueError("VitalPreset needs positive frequencies and nonnegative linear amplitudes with at least one audible partial")
            base = partials.meta.get("base_freq", p.base_freq)
            if not np.isfinite(base) or base <= 0:
                raise ValueError("VitalPreset needs a finite, positive base_freq")
            timbre = Timbre(partials_hz=hz, amplitudes=amps, base_freq=float(base),
                            matching_method="explicit", matched_tuning=(hz / base).tolist())
        else:
            if input is None:
                raise ValueError("VitalPreset needs a tuning or partials and amplitudes before writing")
            row = self._row(input, p.row, "tuning")
            ratios = np.unique(row[np.isfinite(row) & (row > 0)])
            if not ratios.size:
                raise ValueError("VitalPreset needs at least one positive tuning ratio")
            timbre = timbre_from_ratios(ratios, matching_method=p.matching, base_freq=p.base_freq)

        extra = {}
        if p.kind == "ensemble" and signal is not None:
            raw = self._row(signal, p.signalRow, "signal")
            sfreq = signal.meta.get("sfreq")
            if sfreq is None or not np.isfinite(sfreq) or sfreq <= 0:
                raise ValueError("VitalPreset ensemble signal needs finite, positive sfreq metadata")
            if raw.size < 4 or not np.all(np.isfinite(raw)):
                raise ValueError("VitalPreset ensemble signal needs at least four finite samples")
            extra = {"signal": raw, "sf": float(sfreq)}

        folder = os.path.abspath(os.path.expanduser(p.folder or "presets"))
        os.makedirs(folder, exist_ok=True)
        if p.kind == "ensemble":
            path = os.path.join(folder, name)
            result = KINDS[p.kind](timbre, path, bundle_name=name, **extra)
            files = {}
            for voice, value in result.items():
                if isinstance(value, dict):
                    files.update({f"{voice}/{kind}": filename for kind, filename in value.items()})
                else:
                    files[voice] = value
        else:
            result = KINDS[p.kind](timbre, os.path.join(folder, name + ".vital"), preset_name=name)
            path, files = result["vital"], result
        self.written = {"path": path, "files": files}
