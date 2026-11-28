import goofi
import numpy as np


class TextureBytes(goofi.GraphicsNode):
    """An 8-bit picture with no input: RGBA, or RGB where `alpha` is off.

    Blue says that the picture before this one left in device memory.
    """

    OUTPUTS = {'out': goofi.DataType.TEXTURE}
    PRODUCER = True
    PARAMS = {'picture': {'alpha': goofi.BoolParam(True), 'width': goofi.NumParam(3, 1, 64, int=True)}}

    def setup(self):
        self.left_shared = False

    def process(self):
        image = np.zeros((2, self.params.picture.width, 4), dtype=np.uint8)
        image[:] = [51, 102, 255 if self.left_shared else 0, 128]
        out = goofi.Texture(image if self.params.picture.alpha else image[..., :3])
        self.left_shared = out.shared
        return out
