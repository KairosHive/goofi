# goofi: graphics
import goofi
import numpy as np

class TexturePython(goofi.Node):
    OUTPUTS = {'out': goofi.DataType.TEXTURE}
    PRODUCER = True
    PARAMS = {'image': {'width': goofi.IntParam(3, 1, 32),
                        'red': goofi.FloatParam(0.25, 0.0, 1.0),
                        'invalid': goofi.BoolParam(False),
                        'flip': goofi.PulseParam()}}

    def setup(self):
        self.green = 0.5

    def pulse_image_flip(self):
        self.green = 1.0

    def process(self):
        if self.params.image.invalid:
            return goofi.Texture(np.zeros((2, 2, 2), dtype=np.uint8))
        image = np.zeros((2, self.params.image.width, 4), dtype=np.float32)
        image[:, :, :] = [self.params.image.red, self.green, 0.75, 0.5]
        return goofi.Texture(image[:, ::-1])
