"""Objects — the things in the picture that are not a body, and where each one sits.

Eighty classes EfficientDet was taught: a person, a chair, a cup, a dog. One row per thing, and the
row opens with the box's CENTRE, so the frame is the same shape a point renderer already reads —
`Wake` and `Skeleton` take it with nothing in between, and the width, height and score follow for
whatever wants them. The class each row is of rides as that row's name.
"""

import os
import pathlib
import time
import urllib.request

import mediapipe as mp
import numpy as np
from mediapipe.tasks.python import BaseOptions
from mediapipe.tasks.python import vision

import goofi

STORE = "https://storage.googleapis.com/mediapipe-models/"
MODEL = "object_detector/efficientdet_lite0/float16/latest/efficientdet_lite0.tflite"


class Objects(goofi.Node):
    """Find named things in a picture, and say where each one is."""

    TAGS = ["analysis", "motion", "image", "ml"]
    INPUTS = {"image": goofi.InputSlot(goofi.DataType.ARRAY)}
    OUTPUTS = {"boxes": goofi.DataType.ARRAY}
    PARAMS = {
        "objects": {
            "detections": goofi.IntParam(8, 1, 64, doc="The most things to report at once, best score first."),
            "confidence": goofi.FloatParam(0.4, 0.0, 1.0, doc="How sure the model must be before a thing is reported at all."),
            "only": goofi.StringParam(
                "", doc="Report only these classes, by name, separated by commas — `person, cup`. Empty reports every one."
            ),
        }
    }

    def setup(self):
        self.task = None
        self.built = None
        self.started = time.monotonic()
        self.stamp = -1

    def process(self, image):
        if image is None:
            return None
        p = self.params.objects
        want = (p.detections, round(p.confidence, 3), p.only)
        if want != self.built:
            self.build(p, want)

        frame = picture(image.data)
        self.stamp = max(int((time.monotonic() - self.started) * 1000.0), self.stamp + 1)
        found = self.task.detect_for_video(mp.Image(image_format=mp.ImageFormat.SRGB, data=frame), self.stamp)

        height, width = frame.shape[0], frame.shape[1]
        rows, labels = [], []
        for det in found.detections:
            box, best = det.bounding_box, det.categories[0]
            rows.append(
                (
                    (box.origin_x + box.width * 0.5) / width,
                    (box.origin_y + box.height * 0.5) / height,
                    box.width / width,
                    box.height / height,
                    best.score,
                )
            )
            labels.append(best.category_name)
        boxes = np.asarray(rows, dtype=np.float32).reshape(-1, 5)
        return boxes, {"channels": {"dim0": labels, "dim1": ["x", "y", "width", "height", "score"]}}

    def build(self, p, want):
        """The detector, held until a param it was built on moves."""
        self.close()
        self.built = want
        allow = [name.strip() for name in p.only.split(",") if name.strip()]
        self.task = vision.ObjectDetector.create_from_options(
            vision.ObjectDetectorOptions(
                base_options=BaseOptions(model_asset_path=str(fetch(MODEL))),
                running_mode=vision.RunningMode.VIDEO,
                max_results=p.detections,
                score_threshold=p.confidence,
                category_allowlist=allow or None,
            )
        )

    def close(self):
        if self.task is not None:
            self.task.close()
            self.task = None

    def stop(self):
        self.close()


def fetch(model):
    """The model file, from goofi's home; the network is asked once and never again."""
    home = pathlib.Path(os.environ.get("GOOFI_HOME") or pathlib.Path.home()) / ".goofi" / "models"
    path = home / model.rsplit("/", 1)[-1]
    if not path.exists():
        home.mkdir(parents=True, exist_ok=True)
        part = path.with_name(f"{path.name}.{os.getpid()}")
        urllib.request.urlretrieve(STORE + model, part)
        part.replace(path)
    return path


def picture(data):
    """Whatever came down the wire, as the 8-bit RGB MediaPipe reads."""
    x = np.asarray(data)
    if x.ndim == 2:
        x = np.repeat(x[..., None], 3, axis=-1)
    if x.ndim != 3 or x.shape[2] < 3:
        raise ValueError(f"Objects reads a picture; this frame is {x.shape}")
    x = x[..., :3]
    if np.issubdtype(x.dtype, np.floating):
        x = np.clip(x, 0.0, 1.0) * 255.0
    return np.ascontiguousarray(x, dtype=np.uint8)
