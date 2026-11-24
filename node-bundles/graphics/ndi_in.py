# goofi: graphics
"""NdiIn — a picture another program sends over the network with NDI, as a texture.

The receiver asks for RGBA whatever the sender encodes, and a frame sync hands over the newest
frame on each tick, so a slow sender repeats its last picture rather than stalling the patch.
"""

import time

import numpy as np
from cyndilib import Finder, Receiver, RecvBandwidth, RecvColorFormat, VideoFrameSync
from cyndilib.wrapper import FourCC

import goofi

# How long a named source may stay unseen after the node starts before it is an error: discovery
# takes a moment on any network.
DISCOVERY = 3.0


class NdiIn(goofi.Node):
    """Receive an NDI video source as a texture."""

    TAGS = ["input", "image"]
    OUTPUTS = {"out": goofi.DataType.TEXTURE}
    PRODUCER = True
    PARAMS = {
        "ndi": {
            "source": goofi.StringParam("", doc="The source to receive, as `HOST (name)`; empty takes the first one found."),
        },
        "common": {
            "max_frequency": goofi.NumParam(60.0, 0.1, 240.0, doc="Rate cap: the frames a second the node asks for."),
        },
    }

    def setup(self):
        self.finder = Finder()
        self.finder.open()
        self.receiver = Receiver(color_format=RecvColorFormat.RGBX_RGBA, bandwidth=RecvBandwidth.highest)
        self.frame = VideoFrameSync()
        self.receiver.frame_sync.set_video_frame(self.frame)
        self.connected = None
        self.stamp = None
        self.started = time.monotonic()

    def process(self):
        name = self.params.ndi.source
        if self.connected is None or (name and name != self.connected):
            if not self.connect(name):
                return None

        self.receiver.frame_sync.capture_video()
        w, h = self.frame.xres, self.frame.yres
        stamp = self.frame.get_timestamp_posix()
        if w == 0 or h == 0 or stamp == self.stamp:
            return None
        self.stamp = stamp

        rows = self.frame.get_array().reshape(h, -1)[:, : w * 4].reshape(h, w, 4)
        if self.frame.get_fourcc() == FourCC.RGBX:
            rows = rows[..., :3]
        return goofi.Texture(np.ascontiguousarray(rows))

    def connect(self, name):
        """Point the receiver at the source `name` names, once discovery has seen it."""
        names = self.finder.get_source_names()
        want = name or (names[0] if names else None)
        if want not in names:
            if time.monotonic() - self.started < DISCOVERY:
                return False
            seen = ", ".join(names) or "none"
            raise ValueError(f"no NDI source {name!r} on the network; visible: {seen}" if name else "no NDI source on the network")
        self.receiver.set_source(self.finder.get_source(want))
        self.connected = want
        self.stamp = None
        return True

    def stop(self):
        self.receiver.disconnect()
        self.finder.close()
