# goofi: graphics
"""VideoFile — a video on disk played back as a texture, on its own clock.

The playhead runs in video seconds at `speed` times real time, so the picture keeps the file's
rate whatever the node's own rate cap is: a tick that lands between two frames repeats the last
one, a tick that lands past several skips them.
"""

import time

import cv2
import numpy as np

import goofi

# Past this many frames the playhead seeks instead of decoding every frame in between.
SKIP_BY_SEEK = 64


class VideoFile(goofi.Node):
    """Play a video file as a texture."""

    TAGS = ["input", "image"]
    OUTPUTS = {"out": goofi.DataType.TEXTURE}
    PRODUCER = True
    PARAMS = {
        "video": {
            "file": goofi.StringParam("", doc="The video file to play."),
            "speed": goofi.NumParam(1.0, 0.0, 8.0, doc="Playback rate as a multiple of real time; 0 pauses."),
            "loop": goofi.BoolParam(True, doc="Start again from `offset` when the file ends; off holds the last frame."),
            "offset": goofi.NumParam(0.0, 0.0, 3600.0, doc="Where a reset starts, in seconds into the file."),
            "reset": goofi.PulseParam(doc="Move the playhead to `offset`."),
        },
        "common": {
            "max_frequency": goofi.NumParam(60.0, 0.1, 240.0, doc="Rate cap: the ticks a second the node asks for."),
        },
    }

    def setup(self):
        self.cap = None
        self.opened = None
        self.fps = 0.0
        self.count = 0
        self.position = 0.0
        # The frame the decoder gives next, and the one the last emit showed.
        self.next = 0
        self.shown = -1
        self.clock = None
        self.pending_reset = False

    def pulse_video_reset(self):
        self.pending_reset = True

    def process(self):
        p = self.params.video
        if p.file != self.opened:
            self.open(p.file)
            self.pending_reset = True
        if self.cap is None:
            return None

        now = time.monotonic()
        if self.pending_reset:
            self.position = min(p.offset, self.duration())
            self.pending_reset = False
        elif self.clock is not None:
            self.position += p.speed * (now - self.clock)
        self.clock = now

        if self.position >= self.duration():
            if not p.loop:
                self.position = self.duration()
                return None
            self.position = p.offset + (self.position - p.offset) % max(self.duration() - p.offset, 1e-9)

        wanted = min(int(self.position * self.fps), self.count - 1)
        if wanted == self.shown:
            return None
        if wanted < self.next or wanted - self.next > SKIP_BY_SEEK:
            self.cap.set(cv2.CAP_PROP_POS_FRAMES, wanted)
            self.next = wanted
        while self.next < wanted:
            self.cap.grab()
            self.next += 1
        ok, frame = self.cap.read()
        if not ok:
            raise ValueError(f"could not decode frame {wanted} of {p.file!r}")
        self.next = wanted + 1
        self.shown = wanted
        return goofi.Texture(np.ascontiguousarray(cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)))

    def duration(self):
        return self.count / self.fps

    def open(self, file):
        """The file the param now names, in place of whatever was open before."""
        self.release()
        self.opened = file
        if not file:
            return
        cap = cv2.VideoCapture(file)
        if not cap.isOpened():
            cap.release()
            raise ValueError(f"no video at {file!r}")
        self.fps = cap.get(cv2.CAP_PROP_FPS) or 30.0
        self.count = int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
        if self.count <= 0:
            cap.release()
            raise ValueError(f"{file!r} has no frames")
        self.cap = cap
        self.next = 0
        self.shown = -1
        self.clock = None

    def release(self):
        if self.cap is not None:
            self.cap.release()
            self.cap = None

    def stop(self):
        self.release()
