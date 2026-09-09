"""A repeatable four-second synthetic biosignal window for the cookbook.

Each output is one independent, complete analysis window at 128 Hz. This is
test data, not a physiological model or a live recording. Shift changes the
three partials from 6/10/15 Hz to 7/11/17 Hz without replacing the node.
"""

import numpy as np
import goofi


class GeometrySignal(goofi.Node):
    """Supply a complete synthetic window without a device or an external file."""

    TAGS = ['generator']
    OUTPUTS = {'out': goofi.DataType.ARRAY}
    PARAMS = {'signal': {'shift': goofi.FloatParam(0.0, 0.0, 1.0)},
              'common': {'autotrigger': goofi.BoolParam(True), 'max_frequency': goofi.FloatParam(3.0, 0.0, 10.0)}}

    def process(self):
        t = np.arange(512)/128
        freq = np.array([6, 10, 15])+self.params.signal.shift*np.array([1, 1, 2])
        wave = np.sum(np.array([1, .7, .45])[:, None]*np.sin(2*np.pi*freq[:, None]*t), axis=0)
        return wave.astype(np.float32), {'sfreq': 128.0}
