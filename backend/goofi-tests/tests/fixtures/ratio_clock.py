import goofi
import numpy as np


class RatioClock(goofi.Node):
    OUTPUTS = {'out': goofi.DataType.ARRAY}
    PARAMS = {'clock': {'time': goofi.FloatParam(0.0, -1000.0, 1000.0)},
              'common': {'autotrigger': goofi.BoolParam(True)}}

    def process(self):
        return {'out': np.array([self.params.clock.time], dtype=np.float32)}
