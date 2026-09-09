"""Segment — which texels are the person, as a mask a picture can be cut with.

`PoseEstimation` answers where a body's joints are; this answers which pixels the body occupies.
That is what puts a figure INSIDE a generated field rather than beside it: the mask multiplies, and
`graphics:Composite` does the rest.

`scene` is the wider question — the twenty-one things DeepLab was taught to tell apart — and
`category` picks which one of them the mask is of.
"""

import os
import pathlib
import time
import urllib.request

import cv2
import mediapipe as mp
import numpy as np
from mediapipe.tasks.python import BaseOptions
from mediapipe.tasks.python import vision

import goofi

SUBJECTS = ["selfie", "scene"]

STORE = "https://storage.googleapis.com/mediapipe-models/"

MODELS = {
    "selfie": "image_segmenter/selfie_segmenter/float16/latest/selfie_segmenter.tflite",
    "scene": "image_segmenter/deeplab_v3/float32/latest/deeplab_v3.tflite",
}


class Segment(goofi.Node):
    """Cut the person, or one named thing, out of the picture as a soft mask."""

    TAGS = ["analysis", "motion", "image"]
    INPUTS = {"image": goofi.InputSlot(goofi.DataType.ARRAY)}
    OUTPUTS = {"mask": goofi.DataType.ARRAY}
    PARAMS = {
        "segment": {
            "subject": goofi.StringParam(
                "selfie", SUBJECTS, doc="`selfie` is the person in front of the camera; `scene` tells twenty-one things apart."
            ),
            "category": goofi.IntParam(
                0, 0, 20, doc="Which of `scene`'s classes the mask is of, counting from the background. `selfie` has only its one."
            ),
            "soften": goofi.FloatParam(0.0, 0.0, 32.0, doc="Blur the mask's edge by this many texels, so a cut does not read as a cut."),
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
        p = self.params.segment
        if p.subject != self.built:
            self.build(p)

        frame = picture(image.data)
        self.stamp = max(int((time.monotonic() - self.started) * 1000.0), self.stamp + 1)
        found = self.task.segment_for_video(mp.Image(image_format=mp.ImageFormat.SRGB, data=frame), self.stamp)

        masks = found.confidence_masks
        mask = np.asarray(masks[min(p.category, len(masks) - 1)].numpy_view(), dtype=np.float32)
        mask = mask.reshape(mask.shape[0], mask.shape[1])
        if p.soften > 0.0:
            width = int(p.soften) * 2 + 1
            mask = cv2.GaussianBlur(mask, (width, width), 0)
        return np.ascontiguousarray(mask), {"classes": len(masks)}

    def build(self, p):
        """The segmenter `subject` names, on the model it needs."""
        self.close()
        self.built = p.subject
        self.task = vision.ImageSegmenter.create_from_options(
            vision.ImageSegmenterOptions(
                base_options=BaseOptions(model_asset_path=str(fetch(MODELS[p.subject]))),
                running_mode=vision.RunningMode.VIDEO,
                output_confidence_masks=True,
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
        raise ValueError(f"Segment reads a picture; this frame is {x.shape}")
    x = x[..., :3]
    if np.issubdtype(x.dtype, np.floating):
        x = np.clip(x, 0.0, 1.0) * 255.0
    return np.ascontiguousarray(x, dtype=np.uint8)
