# goofi: graphics
import goofi

try:
    import torch
except ImportError:
    torch = None


class TextureCuda(goofi.Node):
    """Half the picture on its input, through torch where there is one.

    Green says that the input came in device memory, blue that the last picture left in it.
    """

    INPUTS = {'image': goofi.InputSlot(goofi.DataType.TEXTURE)}
    OUTPUTS = {'out': goofi.DataType.TEXTURE}

    def setup(self):
        self.left_shared = False

    def process(self, image):
        if image is None:
            return None
        pixels = image.data.numpy() if torch is None else image.data.torch()
        half = pixels * 0.5
        half[..., 1] = 1.0 if image.data.shared else 0.0
        half[..., 2] = 1.0 if self.left_shared else 0.0
        out = goofi.Texture(half)
        self.left_shared = out.shared
        return out
