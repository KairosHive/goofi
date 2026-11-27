import goofi
import numpy as np


class TextureTick(goofi.GraphicsNode):
    """The picture on its input, times a gain; the graphics tick calls it."""

    INPUTS = {'image': goofi.InputSlot(goofi.DataType.TEXTURE)}
    OUTPUTS = {'out': goofi.DataType.TEXTURE}
    PARAMS = {'look': {'gain': goofi.NumParam(1.0, 0.0, 4.0), 'fail': goofi.BoolParam(False)}}

    def process(self, image):
        if self.params.look.fail:
            raise RuntimeError('asked to fail')
        if image is None:
            return None
        gain = self.params.look.gain
        return goofi.Texture(image.data.numpy() * np.array([gain, gain, gain, 1.0], dtype=np.float32))
