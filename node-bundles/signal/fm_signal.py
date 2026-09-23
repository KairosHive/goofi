"""Reusable real-time, block-output sinusoidal FM source for signal patches."""
import time
import numpy as np
import goofi


class FmSignal(goofi.Node):
    """Generate a sinusoidal FM signal in real-time sample blocks.

    Carrier and modulator are frequencies in Hz. Index sets phase deviation:
    zero gives a sine wave; increasing it adds sidebands around the carrier.
    Carrier and modulator phases persist between blocks and parameter changes.
    Output is a float32 array in [-1, 1] with sampling-rate metadata. Connect
    it to Buffer, Psd, or Peaks to inspect the spectrum. Choose output/sfreq
    high enough for the sidebands; this source does not prevent aliasing.
    """
    TAGS = ['generator']
    PRODUCER = True
    OUTPUTS = {'out': goofi.DataType.ARRAY}
    PARAMS = {'fm': {'carrier': goofi.FloatParam(18., 1., 100., doc='Carrier frequency in Hz.'),
                     'modulator': goofi.FloatParam(5., .1, 100., doc='Modulator frequency in Hz; sets sideband spacing.'),
                     'index': goofi.FloatParam(1.2, 0., 5., doc='Phase deviation in radians. Zero disables modulation.')},
              'output': {'sfreq': goofi.FloatParam(256., 256., 4096., doc='Output sample rate in Hz. Raise this to avoid aliasing at higher frequencies.')}}

    def setup(self):
        self.last = time.monotonic()
        self.remainder = 0.
        self.carrier_phase = self.modulator_phase = 0.

    def process(self):
        now = time.monotonic()
        rate = float(self.params.output.sfreq)
        count = (now - self.last) * rate + self.remainder
        self.last = now
        n = int(count)
        self.remainder = count - n
        if n == 0:
            return {}
        p = self.params.fm
        t = np.arange(n) / rate
        carrier = self.carrier_phase + 2 * np.pi * p.carrier * t
        modulator = self.modulator_phase + 2 * np.pi * p.modulator * t
        signal = np.sin(carrier + p.index * np.sin(modulator))
        self.carrier_phase = (self.carrier_phase + 2*np.pi*p.carrier*n/rate) % (2*np.pi)
        self.modulator_phase = (self.modulator_phase + 2*np.pi*p.modulator*n/rate) % (2*np.pi)
        return {'out': (signal.astype(np.float32), {'sfreq': rate})}
