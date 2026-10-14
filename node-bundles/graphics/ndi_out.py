"""NdiOut — a texture sent over the network with NDI, for any program on the network to receive.

The texture crosses from the graphics plane as a frame, and goes out as 8-bit RGBA. The sender is
rebuilt when its name or the frame's size changes, because an NDI video frame's format is fixed
while its sender is open. The stream is not clocked: a frame goes out as soon as it arrives.
"""

from fractions import Fraction

import numpy as np
from cyndilib import Sender, VideoSendFrame
from cyndilib.wrapper import FourCC

import goofi


class NdiOut(goofi.Node):
    """Send a texture as an NDI video source."""

    TAGS = ["output", "image"]
    INPUTS = {"input": goofi.InputSlot(goofi.DataType.TEXTURE, required=True)}
    PARAMS = {
        "ndi": {
            "name": goofi.StringParam("goofi", doc="The name receivers list this source by."),
            "fps": goofi.NumParam(60.0, 1.0, 240.0, doc="The frame rate the stream announces; receivers pace playback by it."),
        }
    }

    def setup(self):
        self.sender = None
        self.built = None

    def process(self, input):
        p = self.params.ndi
        x = np.asarray(input.data)
        if x.ndim != 3 or x.shape[2] not in (3, 4):
            raise ValueError(f"NdiOut sends a picture of [H, W, 3] or [H, W, 4], not {list(x.shape)}")
        if np.issubdtype(x.dtype, np.floating):
            x = np.rint(np.clip(x, 0.0, 1.0) * 255.0)
        h, w = x.shape[:2]
        rgba = np.full((h, w, 4), 255, dtype=np.uint8)
        rgba[..., : x.shape[2]] = x

        wanted = (p.name, w, h, p.fps)
        if wanted != self.built:
            self.open(p, w, h)
            self.built = wanted
        self.sender.write_video(rgba.ravel())
        return None

    def open(self, p, w, h):
        """A sender for frames of `w` by `h` under `p.name`, in place of the one before."""
        self.close()
        frame = VideoSendFrame()
        frame.set_resolution(w, h)
        frame.set_frame_rate(Fraction(p.fps).limit_denominator(1001))
        frame.set_fourcc(FourCC.RGBA)
        self.sender = Sender(p.name, clock_video=False, clock_audio=False)
        self.sender.set_video_frame(frame)
        self.sender.open()

    def close(self):
        if self.sender is not None:
            self.sender.close()
            self.sender = None

    def stop(self):
        self.close()
