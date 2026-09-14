"""Upscale new RGB frames with the optional Real-ESRGAN general-x4v3 model."""
from pathlib import Path
import time

import numpy as np
import torch
import goofi


class Model:
    """Run the upstream SRVGGNetCompact architecture with strict checkpoint loading."""
    def __init__(self, path):
        from torch import nn
        self.torch = torch
        if not torch.cuda.is_available():
            raise ValueError('RealESRGAN requires CUDA PyTorch and an NVIDIA GPU')
        # Real-ESRGAN SRVGGNetCompact: 32 body convolutions, 64 features, PReLU.
        # See REAL-ESRGAN-LICENSE.txt for the upstream BSD license.
        class Network(nn.Module):
            def __init__(self):
                super().__init__()
                self.body = nn.ModuleList([nn.Conv2d(3, 64, 3, 1, 1), nn.PReLU(64)])
                for _ in range(32):
                    self.body.extend([nn.Conv2d(64, 64, 3, 1, 1), nn.PReLU(64)])
                self.body.append(nn.Conv2d(64, 48, 3, 1, 1))
                self.upsampler = nn.PixelShuffle(4)

            def forward(self, image):
                result = image
                for layer in self.body:
                    result = layer(result)
                return self.upsampler(result) + torch.nn.functional.interpolate(image, scale_factor=4, mode='nearest')

        checkpoint = torch.load(path, map_location='cpu', weights_only=True)
        state = checkpoint.get('params_ema', checkpoint.get('params'))
        if state is None:
            raise ValueError('Expected a Real-ESRGAN general-x4v3 checkpoint with params or params_ema')
        self.network = Network()
        self.network.load_state_dict(state, strict=True)
        self.network.eval().requires_grad_(False).to(device='cuda', dtype=torch.float16)
        self.precision = 'fp16'

    def process(self, image, precision, scale):
        torch = self.torch
        dtype = torch.float16 if precision == 'fp16' else torch.float32
        if precision != self.precision:
            self.network.to(dtype=dtype)
            self.precision = precision
        with torch.inference_mode():
            source = torch.from_numpy(image.transpose(2, 0, 1).copy()).unsqueeze(0).to(device='cuda', dtype=dtype)
            result = self.network(source)
            if scale == 2:
                result = torch.nn.functional.interpolate(result, scale_factor=0.5, mode='area')
            result = result[0].float().clamp_(0, 1).permute(1, 2, 0).contiguous().cpu().numpy()
        if not np.isfinite(result).all():
            raise ValueError('RealESRGAN produced non-finite pixels')
        return result

    def close(self):
        self.network = None
        self.torch.cuda.empty_cache()


class RealESRGAN(goofi.Node):
    """Restore and upscale RGB images. This can smooth fine textures and change detail."""
    TAGS = ['image', 'ml', 'transform']
    INPUTS = {'image': goofi.InputSlot(goofi.DataType.ARRAY, required=False)}
    OUTPUTS = {'image': goofi.DataType.ARRAY, 'status': goofi.DataType.STRING}
    PARAMS = {
        'runtime': {'state': goofi.StringParam('stop', options=['start', 'pause', 'stop'],
                    doc='Start loads and accepts new frames. Pause retains the model. Stop releases it.')},
        'model': {'weights': goofi.StringParam('', doc='Absolute path to realesr-general-x4v3.pth. Stop before changing.'),
                  'precision': goofi.StringParam('fp16', options=['fp16', 'fp32'],
                    doc='FP16 is faster and uses less memory. Changes apply to the next new frame.')},
        'upscale': {'scale': goofi.IntParam(4, 2, 4, options=[2, 4],
                    doc='The network computes 4x. 2x averages the 4x output on the GPU before transfer.')},
        'common': {'autotrigger': goofi.BoolParam(True),
                   'max_frequency': goofi.FloatParam(60.0, 1.0, 120.0)},
    }

    def setup(self):
        self.model = None
        self.weights = None

    def process(self, image=None):
        state = self.params.runtime.state
        if state not in ('start', 'pause', 'stop'):
            raise ValueError('Set runtime/state to start, pause, or stop')
        if state == 'stop':
            self.stop()
            self.clear_input('image')
            return {'status': 'Stopped — select start to load'}
        if state == 'pause':
            return {'status': 'Paused'}
        weights = self.params.model.weights
        if self.model is not None and weights != self.weights:
            raise ValueError('Stop RealESRGAN before changing model/weights')
        precision, scale = self.params.model.precision, self.params.upscale.scale
        if precision not in ('fp16', 'fp32') or scale not in (2, 4):
            raise ValueError('Use fp16 or fp32 precision and a scale of 2 or 4')
        if self.model is None:
            if not weights or not Path(weights).is_absolute() or not Path(weights).is_file():
                raise ValueError('Set model/weights to the absolute path of realesr-general-x4v3.pth')
            self.model = Model(weights)
            self.weights = weights
        if image is None:
            return None
        source = np.asarray(image.data, dtype=np.float32)
        if source.ndim != 3 or source.shape[2] != 3 or not source.size:
            raise ValueError('RealESRGAN needs an RGB image with shape [height, width, 3]')
        if max(source.shape[:2]) > 1024:
            raise ValueError('RealESRGAN accepts input dimensions up to 1024; resize before this node')
        if not np.isfinite(source).all() or source.min() < 0 or source.max() > 1:
            raise ValueError('RealESRGAN needs finite RGB values in 0..1')
        started = time.perf_counter()
        result = self.model.process(source, precision, scale)
        elapsed = time.perf_counter()-started
        self.clear_input('image')
        meta = {**image.meta, 'channels': {'dim2': ['r', 'g', 'b']},
                'upscale': {'method': 'realesrgan', 'scale': scale, 'precision': precision,
                            'seconds': elapsed, 'width': result.shape[1], 'height': result.shape[0]}}
        return {'image': (result, meta),
                'status': f'{result.shape[1]} x {result.shape[0]} | {precision} | {elapsed*1000:.1f} ms'}

    def stop(self):
        if self.model is not None:
            self.model.close()
        self.model = None
        self.weights = None
