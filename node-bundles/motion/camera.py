"""Camera — what a lens or a video file is looking at, as a frame the rest of the patch reads.

The one door in from a camera. Everything that reads a picture takes it from here, so a device is
opened once however many nodes watch what it sees.
"""

import cv2
import numpy as np

import goofi

SOURCES = ["camera", "file"]


class Camera(goofi.Node):
    """Frames from a capture device or a video file."""

    TAGS = ["input", "image"]
    OUTPUTS = {"out": goofi.DataType.ARRAY}
    PRODUCER = True
    PARAMS = {
        "camera": {
            "source": goofi.StringParam("camera", SOURCES, doc="A live device, or a file on disk."),
            "device": goofi.IntParam(0, 0, 15, doc="Which capture device, counting from the first one."),
            "file": goofi.StringParam("", doc="The video file to read, when `source` is `file`."),
            "width": goofi.IntParam(640, 0, 4096, doc="Width to ask the device for; 0 takes what it offers."),
            "height": goofi.IntParam(480, 0, 4096, doc="Height to ask the device for; 0 takes what it offers."),
            "mirror": goofi.BoolParam(True, doc="Flip left for right, so the picture moves the way you do."),
            "loop": goofi.BoolParam(True, doc="Start a file again when it ends."),
        },
        "common": {
            "max_frequency": goofi.FloatParam(30.0, 0.1, 240.0, doc="Rate cap: the frames a second the node asks for."),
        },
    }

    def setup(self):
        self.cap = None
        self.opened = None

    def process(self):
        p = self.params.camera
        want = (p.source, p.device, p.file, p.width, p.height)
        if want != self.opened:
            self.open(p, want)
        if self.cap is None:
            return None

        ok, frame = self.cap.read()
        if not ok and p.source == "file" and p.loop:
            self.cap.set(cv2.CAP_PROP_POS_FRAMES, 0)
            ok, frame = self.cap.read()
        if not ok:
            return None

        rgb = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
        if p.mirror:
            rgb = rgb[:, ::-1]
        out = np.ascontiguousarray(rgb, dtype=np.float32) / 255.0
        return out, {"channels": {"dim2": ["r", "g", "b"]}}

    def open(self, p, want):
        """The device or file the params now name, in place of whatever was open before."""
        self.release()
        self.opened = want
        target = p.device if p.source == "camera" else p.file
        if p.source == "file" and not p.file:
            return
        cap = cv2.VideoCapture(target)
        if not cap.isOpened():
            cap.release()
            raise ValueError(f"no {p.source} at {target!r}")
        if p.width:
            cap.set(cv2.CAP_PROP_FRAME_WIDTH, p.width)
        if p.height:
            cap.set(cv2.CAP_PROP_FRAME_HEIGHT, p.height)
        self.cap = cap

    def release(self):
        if self.cap is not None:
            self.cap.release()
            self.cap = None

    def stop(self):
        self.release()
