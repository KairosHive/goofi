"""Read a local image as RGB data for a viewer or image reference input."""
from pathlib import Path

import numpy as np
from PIL import Image, ImageOps
import goofi


class ImageFile(goofi.Node):
    """Load PNG, JPEG, WebP and other Pillow image formats from an absolute file path."""
    TAGS = ['image', 'input']
    PRODUCER = True
    OUTPUTS = {'image': goofi.DataType.ARRAY, 'status': goofi.DataType.STRING}
    PARAMS = {
        'file': {'path': goofi.StringParam('', doc='Absolute path to a local image. Paste the path without quotes.'),
                 'reload': goofi.PulseParam(doc='Read the image again. File changes also reload automatically.')},
        'image': {'max_size': goofi.IntParam(1024, 64, 4096, doc='Maximum width or height. Preserve aspect ratio; do not enlarge small images.')},
        'common': {'max_frequency': goofi.FloatParam(1.0, 0.1, 10.0)},
    }

    def setup(self):
        self.key = None
        self.image = None

    def pulse_file_reload(self):
        self.key = None

    def process(self):
        name = self.params.file.path.strip()
        if not name:
            self.key = self.image = None
            return {'status': 'Set file/path to an image file'}
        path = Path(name)
        if not path.is_absolute() or not path.is_file():
            self.key = self.image = None
            raise ValueError('Set file/path to the absolute path of an existing image')
        limit = self.params.image.max_size
        info = path.stat()
        key = (str(path), info.st_mtime_ns, info.st_size, limit)
        if key != self.key:
            self.key = self.image = None
            with Image.open(path) as source:
                corrected = ImageOps.exif_transpose(source).convert('RGBA')
                corrected.thumbnail((limit, limit), Image.Resampling.LANCZOS)
                background = Image.new('RGBA', corrected.size, 'white')
                rgb = Image.alpha_composite(background, corrected).convert('RGB')
                self.image = np.ascontiguousarray(np.asarray(rgb, dtype=np.float32)/255.0)
            self.key = key
        height, width = self.image.shape[:2]
        return {'image': (self.image, {'channels': {'dim2': ['r', 'g', 'b']},
                                      'image_file': {'path': str(path), 'width': width, 'height': height}}),
                'status': f'{path.name} | {width} x {height} RGB'}
