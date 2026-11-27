"""VideoFile — a video on disk played back as a texture, on the graphics tick.

The playhead runs in video seconds at `speed` times real time, so the picture keeps the file's
rate whatever the frame rate of the patch is: a tick that lands between two frames keeps the last
one, a tick that lands past several skips them. A thread of the node's own opens the file and
decodes; the tick takes the newest frame it has and never waits for the decoder.
"""

import threading
import time

import cv2

import goofi

# Past this many frames the playhead seeks instead of decoding every frame in between.
SKIP_BY_SEEK = 64


class Decoder(threading.Thread):
    """One open file: decodes the frame the playhead last asked for, and holds it until taken."""

    def __init__(self, file):
        super().__init__(daemon=True)
        self.file = file
        self.fps = 0.0
        self.count = 0  # frames in the file; 0 until it is open
        self.error = None
        self.lock = threading.Lock()
        self.asked = threading.Event()
        self.wanted = -1
        self.ready = None
        self.closed = False
        self.start()

    def want(self, index):
        if index != self.wanted:
            self.wanted = index
            self.asked.set()

    def take(self):
        """The newest decoded frame not yet taken, as RGBA bytes, or None."""
        with self.lock:
            frame, self.ready = self.ready, None
        return frame

    def close(self):
        self.closed = True
        self.asked.set()

    def run(self):
        cap = cv2.VideoCapture(self.file)
        try:
            fps, count = cap.get(cv2.CAP_PROP_FPS) or 30.0, int(cap.get(cv2.CAP_PROP_FRAME_COUNT))
            if not cap.isOpened() or count <= 0:
                self.error = f"{self.file!r} has no frames" if cap.isOpened() else f"no video at {self.file!r}"
                return
            self.fps, self.count = fps, count
            following, shown = 0, -1  # the frame the decoder gives next, and the one last decoded
            while True:
                self.asked.wait()
                self.asked.clear()
                if self.closed:
                    return
                wanted = self.wanted
                if wanted == shown:
                    continue
                if wanted < following or wanted - following > SKIP_BY_SEEK:
                    cap.set(cv2.CAP_PROP_POS_FRAMES, wanted)
                    following = wanted
                while following < wanted:
                    cap.grab()
                    following += 1
                ok, frame = cap.read()
                following = wanted + 1
                if not ok and wanted == 0:
                    self.error = f"could not decode {self.file!r}"
                    return
                if not ok:
                    # A container can count more frames than it holds: the file ends here.
                    self.count = min(self.count, wanted)
                    continue
                shown = wanted
                frame = cv2.cvtColor(frame, cv2.COLOR_BGR2RGBA)
                with self.lock:
                    self.ready = frame
        finally:
            cap.release()


class VideoFile(goofi.GraphicsNode):
    """Play a video file as a texture."""

    TAGS = ["input", "image"]
    OUTPUTS = {"out": goofi.DataType.TEXTURE}
    PARAMS = {
        "video": {
            "file": goofi.StringParam("", doc="The video file to play."),
            "speed": goofi.NumParam(1.0, 0.0, 8.0, doc="Playback rate as a multiple of real time; 0 pauses."),
            "loop": goofi.BoolParam(True, doc="Start again from `offset` when the file ends; off holds the last frame."),
            "offset": goofi.NumParam(0.0, 0.0, 3600.0, doc="Where a reset starts, in seconds into the file."),
            "reset": goofi.PulseParam(doc="Move the playhead to `offset`."),
        },
    }

    def setup(self):
        self.decoder = None
        self.opened = None
        self.position = 0.0
        self.clock = None
        self.pending_reset = False

    def pulse_video_reset(self):
        self.pending_reset = True

    def process(self):
        p = self.params.video
        if p.file != self.opened:
            self.release()
            self.opened = p.file
            self.decoder = Decoder(p.file) if p.file else None
            self.clock = None
            self.pending_reset = True
        d = self.decoder
        if d is None:
            return None
        if d.error:
            raise ValueError(d.error)
        if not d.count:
            return None

        duration = d.count / d.fps
        now = time.monotonic()
        if self.pending_reset:
            self.position = min(p.offset, duration)
            self.pending_reset = False
        elif self.clock is not None:
            self.position += p.speed * (now - self.clock)
        self.clock = now
        if self.position >= duration:
            if p.loop:
                self.position = p.offset + (self.position - p.offset) % max(duration - p.offset, 1e-9)
            else:
                self.position = duration

        d.want(min(int(self.position * d.fps), d.count - 1))
        frame = d.take()
        return None if frame is None else goofi.Texture(frame)

    def release(self):
        if self.decoder is not None:
            self.decoder.close()
            self.decoder = None

    def stop(self):
        self.release()
