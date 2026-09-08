"""Flow — which way the picture moved, with no model of what it was that moved.

`PoseEstimation` finds a body and reports the points it knows the names of. This reports the motion
of every texel and knows nothing: a curtain, a crowd, water, a hand out of frame. That makes it the
model-free half of the bundle, and it is what a displacement wants — the field goes to
`graphics:ArrayIn` and out of it as a texture that bends another picture.

Motion is reported the way `PoseEstimation` reports it, in frame-widths a second, so the two are
the same units wherever a patch mixes them.
"""

import time

import cv2
import numpy as np

import goofi

METHODS = ["dis", "farneback"]
GRADES = ["ultrafast", "fast", "medium"]

PRESETS = {
    "ultrafast": cv2.DISOPTICAL_FLOW_PRESET_ULTRAFAST,
    "fast": cv2.DISOPTICAL_FLOW_PRESET_FAST,
    "medium": cv2.DISOPTICAL_FLOW_PRESET_MEDIUM,
}


class Flow(goofi.Node):
    """How every part of the picture moved since the picture before it."""

    TAGS = ["analysis", "motion", "image"]
    INPUTS = {"image": goofi.InputSlot(goofi.DataType.ARRAY)}
    OUTPUTS = {"flow": goofi.DataType.ARRAY}
    PARAMS = {
        "flow": {
            "method": goofi.StringParam(
                "dis", METHODS, doc="`dis` is fast and reads large movement well; `farneback` is slower and smoother."
            ),
            "grade": goofi.StringParam("fast", GRADES, doc="How hard `dis` looks. Each step up costs, and `farneback` ignores it."),
            "scale": goofi.FloatParam(
                0.5, 0.1, 1.0, doc="How far the picture is shrunk before the search. This also sets the size of the field that comes out."
            ),
            "smooth": goofi.FloatParam(0.4, 0.0, 0.99, doc="How much of the last field to keep, so the flow walks rather than flickers."),
            "floor": goofi.FloatParam(0.0, 0.0, 1.0, doc="Motion slower than this is reported as none, which quiets a grainy picture."),
        }
    }

    def setup(self):
        self.prev = None
        self.prev_t = None
        self.held = None
        self.algo = None
        self.graded = None

    def process(self, image):
        if image is None:
            return None
        p = self.params.flow
        gray = shrink(image.data, p.scale)
        now = time.monotonic()

        if self.prev is None or self.prev.shape != gray.shape:
            self.prev, self.prev_t, self.held = gray, now, None
            return None

        raw = self.between(self.prev, gray, p)
        self.prev = gray

        # Pixels between two pictures says nothing without knowing how far apart they were. In
        # frame-widths a second it is the same reading `PoseEstimation` gives for a landmark.
        span = max(now - self.prev_t, 1e-3)
        self.prev_t = now
        field = raw / (np.array([gray.shape[1], gray.shape[0]], np.float32) * span)

        if p.floor > 0.0:
            field = np.where(np.hypot(field[..., 0], field[..., 1])[..., None] < p.floor, 0.0, field)
        if self.held is not None and self.held.shape == field.shape:
            field = p.smooth * self.held + (1.0 - p.smooth) * field
        self.held = field.astype(np.float32)
        return self.held, {"channels": {"dim2": ["x", "y"]}}

    def between(self, before, after, p):
        """The two pictures, and where each part of the first one went."""
        if p.method == "farneback":
            return cv2.calcOpticalFlowFarneback(before, after, None, 0.5, 3, 15, 3, 5, 1.2, 0)
        if self.algo is None or self.graded != p.grade:
            self.algo, self.graded = cv2.DISOpticalFlow_create(PRESETS[p.grade]), p.grade
        return self.algo.calc(before, after, None)


def shrink(data, scale):
    """The picture as the one 8-bit gray plane a flow search reads, at the size it is searched at."""
    x = np.asarray(data)
    if x.ndim == 3:
        x = x[..., :3].mean(axis=2) if x.shape[2] >= 3 else x[..., 0]
    if x.ndim != 2:
        raise ValueError(f"Flow reads a picture; this frame is {np.asarray(data).shape}")
    if np.issubdtype(x.dtype, np.floating):
        x = np.clip(x, 0.0, 1.0) * 255.0
    gray = np.ascontiguousarray(x, dtype=np.uint8)
    if scale >= 1.0:
        return gray
    return cv2.resize(gray, (max(int(gray.shape[1] * scale), 8), max(int(gray.shape[0] * scale), 8)), interpolation=cv2.INTER_AREA)
