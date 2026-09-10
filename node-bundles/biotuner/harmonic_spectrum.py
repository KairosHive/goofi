"""HarmonicSpectrum measures harmonic similarity across a signal's full spectrum.

Connect a raw time series with `sfreq` metadata. Use Buffer to supply at least
`int(sfreq / precision)` samples per row. Short windows produce no output.
The last axis is time; all leading axes and their labels stay intact.

The spectrum is H(f), weighted by spectral power. It does not include phase
coupling or resonance. `peaks` contains frequencies in Hz and can feed Tuning,
Harmonicity, BioColors, or BioElements. `peakValues` contains H(f) scores, not
the power in dB that Peaks supplies on its `amps` output.

Flat or non-finite rows produce NaN values. Peak outputs have a fixed width,
with NaN padding when fewer peaks are found. Analysis uses 10..512 frequency
bins because the summary needs several bins and the matrix cost is quadratic.
"""

import numpy as np
from biotuner.harmonic_spectrum import compute_harmonic_spectrum, compute_frequency_and_psd, apply_power_law_remove
import goofi


COMPLEXITY = ["flatness", "entropy", "spread", "higuchi"]


class HarmonicSpectrum(goofi.Node):
    """A signal's harmonicity spectrum, peak frequencies, and complexity.

    Input:
      input  raw time series with sfreq metadata; the last axis is time.
             Use Buffer for at least int(sfreq / precision) samples per row.
             Short windows wait; flat or non-finite rows return NaN.

    Connect peaks to Tuning or Harmonicity. peakValues contains harmonicity
    scores, not the power in dB used by Tuning's diss_curve method.

    Outputs:
      analysis     one table with all outputs below from the same input window;
                   connect directly to HarmonicObservatory.analysis
      spectrum     H(f), with frequency labels on the last axis
      freqs        the shared frequency grid in Hz, shape [F]
      peaks        peak frequencies in Hz, shape [..., n_peaks]
      peakValues   H(f) at each detected peak, shape [..., n_peaks]
      matrix       frequency-pair similarity, shape [..., F, F]
      activation   matrix * power[:, None] * power[None, :]
      power        min-subtracted PSD normalized to unit sum, as used by H(f)
      waveform     the exact analysis input window
      harmonicity  mean H(f), one value per input row
      complexity   flatness, entropy, spread in Hz, and Higuchi dimension
    """

    TAGS = ["analysis"]
    INPUTS = {"input": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {name: goofi.DataType.ARRAY for name in (
        "spectrum", "freqs", "peaks", "peakValues", "matrix", "harmonicity", "complexity", "activation", "power", "waveform"
    )}
    OUTPUTS["analysis"] = goofi.DataType.TABLE
    PARAMS = {
        "spectrum": {
            "f_min": goofi.FloatParam(2.0, 0.1, 1000.0, doc="Lowest analysis frequency, in Hz."),
            "f_max": goofi.FloatParam(30.0, 1.0, 1000.0, doc="Highest analysis frequency, at most half of sfreq."),
            "precision": goofi.FloatParam(0.5, 0.01, 10.0, doc="Frequency resolution in Hz. Supply at least sfreq / precision samples."),
            "n_peaks": goofi.IntParam(5, 1, 10, doc="Number of peaks to return, with NaN padding."),
            "smoothness": goofi.FloatParam(1.0, 0.0, 10.0, doc="Gaussian smoothing of H(f), in bins. Zero disables it."),
            "power_law_remove": goofi.BoolParam(False, doc="Subtract a fitted power law from the PSD before analysis."),
            "kernel": goofi.StringParam("harmsim", ["harmsim", "subharm_tension"],
                                       doc="Pair similarity: harmonic ratios or one minus subharmonic tension."),
        },
        "subharmonic": {
            "n_harm": goofi.IntParam(10, 1, 20, doc="Harmonics compared by the subharm_tension kernel."),
            "delta_lim": goofi.FloatParam(20.0, 1.0, 300.0, doc="Largest subharmonic time difference counted, in ms."),
        },
    }

    def process(self, input):
        p = self.params.spectrum
        x = np.asarray(input.data, dtype=np.float64)
        if x.ndim == 0:
            raise ValueError("HarmonicSpectrum needs a time series")
        sfreq = input.meta.get("sfreq")
        if sfreq is None or not np.isfinite(sfreq) or sfreq <= 0:
            raise ValueError("HarmonicSpectrum needs finite, positive `sfreq` metadata")
        if not 0 < p.f_min < p.f_max <= sfreq / 2:
            raise ValueError("Use 0 < f_min < f_max <= sfreq / 2")
        nfft = int(sfreq / p.precision)
        if nfft < 2:
            raise ValueError("Reduce precision to keep at least two samples per segment")
        freqs = np.fft.rfftfreq(nfft, 1.0 / sfreq)
        freqs = freqs[(freqs >= p.f_min) & (freqs <= p.f_max)]
        width = freqs.size
        if not 10 <= width <= 512:
            raise ValueError("Use a band and precision that give 10..512 frequency bins")
        if x.shape[-1] < nfft:
            return None

        lead = x.shape[:-1]
        rows = x.reshape(-1, x.shape[-1])
        spectra = np.full((len(rows), width), np.nan)
        matrices = np.full((len(rows), width, width), np.nan)
        powers = np.full((len(rows), width), np.nan)
        peaks = np.full((len(rows), p.n_peaks), np.nan)
        values = np.full_like(peaks, np.nan)
        means = np.full(len(rows), np.nan)
        complexity = np.full((len(rows), len(COMPLEXITY)), np.nan)
        kernel_params = {}
        if p.kernel == "subharm_tension":
            kernel_params = {
                "n_harms": self.params.subharmonic.n_harm,
                "delta_lim": self.params.subharmonic.delta_lim,
            }
        for i, row in enumerate(rows):
            if not np.all(np.isfinite(row)) or np.ptp(row) == 0:
                continue
            # Some complexity measures are undefined for a flat spectrum. Keep NaN
            # for those measures; do not replace a missing measurement with zero.
            with np.errstate(divide="ignore", invalid="ignore"):
                _, spectrum, matrix, summary = compute_harmonic_spectrum(
                    row, p.precision, fs=float(sfreq), fmin=p.f_min, fmax=p.f_max,
                    smoothness_harm=p.smoothness, power_law_remove=p.power_law_remove,
                    harmonic_kernel=p.kernel, harmonic_kernel_params=kernel_params,
                    n_peaks=p.n_peaks, legacy_self_pair_subtract=False,
                )
            pf, psd = compute_frequency_and_psd(row, p.precision, smoothness=1, fs=float(sfreq), noverlap=1, fmin=p.f_min, fmax=p.f_max)
            clean = apply_power_law_remove(pf, psd, p.power_law_remove)
            clean = np.maximum(clean - np.min(clean), 0)
            powers[i] = clean / clean.sum() if clean.sum() > 0 else np.zeros_like(clean)
            spectra[i] = spectrum
            matrices[i] = matrix
            found = np.asarray(summary["peaks"])[:p.n_peaks]
            indices = np.asarray(summary["peak_indices"], dtype=int)[:p.n_peaks]
            peaks[i, :found.size] = found
            values[i, :indices.size] = spectrum[indices]
            means[i] = summary["avg"]
            complexity[i] = [summary[name] for name in COMPLEXITY]

        meta = input.drop_axis(-1)
        axes = meta.get("channels", {})
        last = f"dim{x.ndim - 1}"
        hz = freqs.tolist()
        spectrum_meta = {**meta, "channels": {**axes, last: hz}}
        matrix_meta = {**meta, "channels": {**axes, last: hz, f"dim{x.ndim}": hz}}
        complexity_meta = {**meta, "channels": {**axes, last: COMPLEXITY}}
        outputs = {
            "activation": ((matrices * powers[:, :, None] * powers[:, None, :]).reshape(lead + (width, width)).astype(np.float32), matrix_meta),
            "power": (powers.reshape(lead + (width,)).astype(np.float32), spectrum_meta),
            "waveform": (x.astype(np.float32), input.meta),
            "spectrum": (spectra.reshape(lead + (width,)).astype(np.float32), spectrum_meta),
            "freqs": (freqs.astype(np.float32), {"channels": {"dim0": hz}}),
            "peaks": (peaks.reshape(lead + (p.n_peaks,)).astype(np.float32), meta),
            "peakValues": (values.reshape(lead + (p.n_peaks,)).astype(np.float32), meta),
            "matrix": (matrices.reshape(lead + (width, width)).astype(np.float32), matrix_meta),
            "harmonicity": (means.reshape(lead or (1,)).astype(np.float32), meta),
            "complexity": (complexity.reshape(lead + (len(COMPLEXITY),)).astype(np.float32), complexity_meta),
        }
        # Both interfaces use the same results, including each array's metadata.
        outputs["analysis"] = {name: goofi.Data(*value) for name, value in outputs.items()}
        return outputs
