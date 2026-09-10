"""RatioSequence: timed ratio steps and pitch glides for harmonic modulation.

One [2,N] transition ARRAY contains the two endpoint ratio rows. Mix, step,
phase and selected voice are metadata. Feed the same packet to HarmonicMorph
and HarmonicModes. Each consumer derives the current state from these endpoints.
The optional clock input contains one elapsed time in seconds. Without it the
node uses a monotonic clock. Frame rate does not determine sequence speed.
"""

from fractions import Fraction
import time

import numpy as np
import goofi


def ratios_from_text(text, limit, name):
    tokens = [s.strip() for s in text.split(',') if s.strip()]
    if not 1 <= len(tokens) <= limit:
        raise ValueError(f'{name} needs between 1 and {limit} ratios')
    try:
        values = np.array([float(Fraction(s)) for s in tokens], dtype=np.float64)
    except (ValueError, ZeroDivisionError, OverflowError) as e:
        raise ValueError(f'{name} needs positive decimals or fractions') from e
    if not np.all(np.isfinite(values) & (values >= 0.0001) & (values <= 10000.0)):
        raise ValueError(f'{name} ratios must be finite and between 0.0001 and 10000')
    return values


class RatioSequence(goofi.Node):
    """Emit successive ratios, with an independent hold and glide in each step.

    Sequence order is retained. A glide occupies the end of each step and joins
    its ratio to the next in log frequency. The loop joins the last to the first.
    Ping-pong reverses without repeating its endpoints. Pause holds the current
    position; reset returns to the first step. Editing ratios or direction also
    restarts. A clock source change or backward clock jump re-anchors at step one.
    Keep anchor ratios outside the moving range to avoid component crossings in
    downstream nodes that sort or merge equal frequencies.

    transition contains the two endpoint ratio rows and their eased mix in metadata. Connect it to HarmonicModes.transition for smooth plate
    mode motion. chord.state selects a single ratio or the full anchor chord;
    its endpoint amplitudes are uniform and phases are zero.
    """

    TAGS = ['generator', 'music']
    INPUTS = {'clock': goofi.InputSlot(goofi.DataType.ARRAY, required=False)}
    OUTPUTS = {'transition': goofi.DataType.ARRAY}
    PARAMS = {
        'sequence': {
            'ratios': goofi.StringParam('9/8, 6/5, 5/4, 4/3, 7/5, 3/2, 5/3, 7/4', doc='Ordered steps, as positive ratios or fractions. At most 64.'),
            'seconds': goofi.FloatParam(2.5, 0.05, 30.0, doc='Seconds per step, including its glide. Steps faster than the update rate can be skipped.'),
            'glide': goofi.FloatParam(0.7, 0.0, 1.0, doc='Fraction of each step used for a smooth pitch glide. Zero gives immediate steps.'),
            'direction': goofi.StringParam('forward', ['forward', 'ping-pong'], doc='Loop forward or reverse at the endpoints.'),
            'running': goofi.BoolParam(True, doc='Off holds the current ratio and position.'),
            'reset': goofi.PulseParam(doc='Return to the first ratio and start its hold again.'),
        },
        'chord': {
            'state': goofi.StringParam('single ratio', ['single ratio', 'anchor chord'], doc='Transition endpoints use one ratio or the full anchor chord with its selected voice replaced.'),
            'anchors': goofi.StringParam('1, 9/8, 2', doc='Fixed chord before one voice is replaced by the moving ratio. At most 32 components.'),
            'voice': goofi.IntParam(1, 0, 31, doc='Zero-based component replaced in tuning. Other components stay fixed.'),
        },
        'common': {'autotrigger': goofi.BoolParam(True), 'max_frequency': goofi.FloatParam(30.0, 0.0, 120.0)},
    }

    def setup(self):
        self.position = 0.0
        self.previous = None
        self.signature = None
        self.external = False
        self.reset_pending = False

    def pulse_sequence_reset(self):
        self.reset_pending = True

    def process(self, clock=None):
        p = self.params.sequence
        ratios = ratios_from_text(p.ratios, 64, 'sequence')
        tuning = ratios_from_text(self.params.chord.anchors, 32, 'anchors')
        voice = self.params.chord.voice
        if not 0 <= voice < len(tuning):
            raise ValueError('voice must select an existing anchor component')
        seconds, glide = float(p.seconds), float(p.glide)
        if not np.isfinite(seconds) or seconds < 0.05 or not np.isfinite(glide) or not 0 <= glide <= 1:
            raise ValueError('seconds must be at least 0.05 and glide must be between 0 and 1')
        if clock is None:
            now = time.monotonic()
        else:
            value = np.asarray(clock.data).ravel()
            if value.size != 1 or not np.isfinite(value[0]):
                raise ValueError('clock needs one finite time in seconds')
            now = float(value[0])
        signature = (tuple(ratios), p.direction)
        restart = (self.reset_pending or self.previous is None or signature != self.signature
                   or (clock is not None) != self.external or now < self.previous)
        span = len(ratios) if p.direction == 'forward' else max(1, 2*(len(ratios)-1))
        position = 0.0 if restart else self.position
        if not restart and p.running:
            advance = (now-self.previous)/seconds
            if not np.isfinite(advance):
                raise ValueError('clock step is too large')
            position = (position+advance) % span
        self.position, self.previous = position, now
        self.signature, self.external, self.reset_pending = signature, clock is not None, False

        def index(step):
            step %= span
            return step if step < len(ratios) else span-step

        step = int(position)
        current, target = index(step), index(step+1)
        phase = position-step
        amount = float(np.clip((phase-(1.0-glide))/glide, 0, 1)) if glide else 0.0
        amount = amount*amount*(3.0-2.0*amount)
        if self.params.chord.state == 'single ratio':
            endpoints = np.array([[ratios[current]], [ratios[target]]], dtype=np.float32)
            voice = 0
        else:
            endpoints = np.tile(tuning, (2, 1)).astype(np.float32)
            endpoints[:, voice] = ratios[current], ratios[target]
        return {'transition': (endpoints, {'mix': amount, 'step': current+1,
                'phase': phase, 'voice': voice, 'channels': {'dim0': ['input', 'target']}})}
