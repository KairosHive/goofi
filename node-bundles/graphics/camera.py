# goofi: graphics
"""Camera — what a lens or a video file is looking at, as a texture on the graphics tick.

The one door in from a camera. Everything that reads a picture takes it from here, so a device is
opened once however many nodes watch what it sees. A thread of the node's own opens the device
and reads it at the device's rate; the tick takes the newest frame and never waits for the lens.
"""

import threading
import time

import cv2

import goofi

SOURCES = ["camera", "file"]
# How long the reader waits before it asks a source that gave no frame again.
RETRY = 0.05


class Reader(threading.Thread):
    """One open device or file: reads frame after frame, and holds the newest until taken."""

    def __init__(self, source, target, width, height):
        super().__init__(daemon=True)
        self.source, self.target, self.width, self.height = source, target, width, height
        self.mirror = True
        self.loop = True
        self.error = None
        self.lock = threading.Lock()
        self.ready = None
        self.closed = False
        self.start()

    def take(self):
        """The newest frame not yet taken, as RGBA bytes, or None."""
        with self.lock:
            frame, self.ready = self.ready, None
        return frame

    def run(self):
        cap = cv2.VideoCapture(self.target)
        try:
            if not cap.isOpened():
                self.error = f"no {self.source} at {self.target!r}"
                return
            if self.width:
                cap.set(cv2.CAP_PROP_FRAME_WIDTH, self.width)
            if self.height:
                cap.set(cv2.CAP_PROP_FRAME_HEIGHT, self.height)
            # A device paces its own reads; a file is read at the rate it was filmed at.
            period = 1.0 / (cap.get(cv2.CAP_PROP_FPS) or 30.0) if self.source == "file" else 0.0
            due = time.monotonic()
            while not self.closed:
                ok, frame = cap.read()
                if not ok and self.source == "file" and self.loop:
                    cap.set(cv2.CAP_PROP_POS_FRAMES, 0)
                    ok, frame = cap.read()
                if not ok:
                    time.sleep(RETRY)
                    continue
                if self.mirror:
                    frame = cv2.flip(frame, 1)
                frame = cv2.cvtColor(frame, cv2.COLOR_BGR2RGBA)
                with self.lock:
                    self.ready = frame
                if period:
                    due = max(due + period, time.monotonic() - period)
                    time.sleep(max(0.0, due - time.monotonic()))
        finally:
            cap.release()


class Camera(goofi.Node):
    """Frames from a capture device or a video file."""

    TAGS = ["input", "image"]
    OUTPUTS = {"out": goofi.DataType.TEXTURE}
    PARAMS = {
        "camera": {
            "source": goofi.StringParam("camera", SOURCES, doc="A live device, or a file on disk."),
            "device": goofi.NumParam(0, 0, 15, int=True, show=("source", ["camera"]), doc="Which capture device, counting from the first one."),
            "file": goofi.StringParam("", show=("source", ["file"]), doc="The video file to read."),
            "width": goofi.NumParam(640, 0, 4096, int=True, show=("source", ["camera"]), doc="Width to ask the device for; 0 takes what it offers."),
            "height": goofi.NumParam(480, 0, 4096, int=True, show=("source", ["camera"]), doc="Height to ask the device for; 0 takes what it offers."),
            "mirror": goofi.BoolParam(True, doc="Flip left for right, so the picture moves the way you do."),
            "loop": goofi.BoolParam(True, show=("source", ["file"]), doc="Start the file again when it ends."),
        },
    }

    def setup(self):
        self.reader = None
        self.opened = None

    def process(self):
        p = self.params.camera
        want = (p.source, p.device, p.file, p.width, p.height)
        if want != self.opened:
            self.release()
            self.opened = want
            if p.source == "camera":
                self.reader = Reader("camera", p.device, p.width, p.height)
            elif p.file:
                self.reader = Reader("file", p.file, 0, 0)
        r = self.reader
        if r is None:
            return None
        if r.error:
            raise ValueError(r.error)
        r.mirror, r.loop = p.mirror, p.loop
        frame = r.take()
        return None if frame is None else goofi.Texture(frame)

    def release(self):
        if self.reader is not None:
            self.reader.closed = True
            # The device is free before the next reader opens it: one frame's wait at most.
            self.reader.join(1.0)
            self.reader = None

    def stop(self):
        self.release()
