"""PoseEstimation — where a body, a hand or a face is in the picture, and how fast it is moving.

One MediaPipe vision task per `mode`, and every mode answers the same two frames: `positions`, one
row per point in the picture's own box, and `velocities`, those same rows in units a second. That
pair is what a field shader takes, so `graphics:Wake` wires straight off this node — and so does
anything else that draws moving points.

The model a mode needs is a file Google publishes rather than one the wheel carries. It is fetched
once into `$GOOFI_HOME/.goofi/models/` and read from there afterwards, so the first run of a mode
wants the network and no run after it does.
"""

import os
import pathlib
import time
import urllib.request
from collections import namedtuple

import mediapipe as mp
import numpy as np
from mediapipe.tasks.python import BaseOptions
from mediapipe.tasks.python import vision

import goofi

# `knobs` maps a task's own option name onto the param that feeds it, which is the whole reason six
# tasks that each spell "how sure" differently need no branch anywhere below.
Task = namedtuple("Task", "cls model call parts knobs")

STORE = "https://storage.googleapis.com/mediapipe-models/"

TASKS = {
    "pose": Task(
        "PoseLandmarker",
        "pose_landmarker/pose_landmarker_lite/float16/1/pose_landmarker_lite.task",
        "detect_for_video",
        ["pose_landmarks"],
        {
            "num_poses": "detections",
            "min_pose_detection_confidence": "confidence",
            "min_pose_presence_confidence": "confidence",
            "min_tracking_confidence": "tracking",
        },
    ),
    "hands": Task(
        "HandLandmarker",
        "hand_landmarker/hand_landmarker/float16/1/hand_landmarker.task",
        "detect_for_video",
        ["hand_landmarks"],
        {
            "num_hands": "detections",
            "min_hand_detection_confidence": "confidence",
            "min_hand_presence_confidence": "confidence",
            "min_tracking_confidence": "tracking",
        },
    ),
    "gesture": Task(
        "GestureRecognizer",
        "gesture_recognizer/gesture_recognizer/float16/1/gesture_recognizer.task",
        "recognize_for_video",
        ["hand_landmarks"],
        {
            "num_hands": "detections",
            "min_hand_detection_confidence": "confidence",
            "min_hand_presence_confidence": "confidence",
            "min_tracking_confidence": "tracking",
        },
    ),
    "face": Task(
        "FaceLandmarker",
        "face_landmarker/face_landmarker/float16/1/face_landmarker.task",
        "detect_for_video",
        ["face_landmarks"],
        {
            "num_faces": "detections",
            "min_face_detection_confidence": "confidence",
            "min_face_presence_confidence": "confidence",
            "min_tracking_confidence": "tracking",
        },
    ),
    "facebox": Task(
        "FaceDetector",
        "face_detector/blaze_face_short_range/float16/1/blaze_face_short_range.tflite",
        "detect_for_video",
        [],
        {"min_detection_confidence": "confidence"},
    ),
    "holistic": Task(
        "HolisticLandmarker",
        "holistic_landmarker/holistic_landmarker/float16/1/holistic_landmarker.task",
        "detect_for_video",
        ["pose_landmarks", "face_landmarks", "left_hand_landmarks", "right_hand_landmarks"],
        {"min_face_detection_confidence": "confidence", "min_pose_detection_confidence": "confidence"},
    ),
}

MODES = list(TASKS)


class PoseEstimation(goofi.Node):
    """Track a body, hands or a face in a picture, and how fast each point moves."""

    TAGS = ["analysis", "motion", "ml"]
    INPUTS = {"image": goofi.InputSlot(goofi.DataType.ARRAY)}
    OUTPUTS = {"positions": goofi.DataType.ARRAY, "velocities": goofi.DataType.ARRAY}
    PARAMS = {
        "pose": {
            "mode": goofi.StringParam(
                "pose",
                MODES,
                doc="What to look for: a body's joints, hands, a hand plus the gesture it makes, a "
                "face's mesh, a face's box and keypoints, or every one of them at once.",
            ),
            "detections": goofi.IntParam(
                1, 1, 4, doc="How many bodies, hands or faces to look for at once. `facebox` and `holistic` set their own count."
            ),
            "confidence": goofi.FloatParam(0.5, 0.0, 1.0, doc="How sure the model must be before it reports anything."),
            "tracking": goofi.FloatParam(
                0.5, 0.0, 1.0, doc="How sure it must stay to keep tracking rather than search again. `facebox` and `holistic` do not track."
            ),
            "smooth": goofi.FloatParam(0.5, 0.0, 0.99, doc="How much of the last velocity to keep, so a jumpy reading walks."),
        }
    }

    def setup(self):
        self.task = None
        self.built = None
        self.prev = None
        self.prev_t = None
        self.held = None
        self.started = time.monotonic()
        self.stamp = -1

    def process(self, image):
        if image is None:
            return None
        p = self.params.pose
        want = (p.mode, p.detections, round(p.confidence, 3), round(p.tracking, 3))
        if want != self.built:
            self.build(p, want)

        frame = picture(image.data)
        # A task refuses a stamp that does not advance, and the fast ones answer twice inside one
        # millisecond, so the counter carries the clock forward rather than reading it raw.
        self.stamp = max(int((time.monotonic() - self.started) * 1000.0), self.stamp + 1)
        found = getattr(self.task, TASKS[p.mode].call)(mp.Image(image_format=mp.ImageFormat.SRGB, data=frame), self.stamp)

        rows, labels = read(found, p.mode, frame.shape[1], frame.shape[0])
        xyz = np.asarray(rows, dtype=np.float32).reshape(-1, 3)
        meta = {"channels": {"dim0": labels, "dim1": ["x", "y", "z"]}}
        if p.mode == "gesture":
            meta["gesture"] = naming(found)
        return {"positions": (xyz, meta), "velocities": (self.moved(xyz, p), meta)}

    def moved(self, xyz, p):
        """How far each row travelled since the last picture, in units a second."""
        now = time.monotonic()
        vel = np.zeros((xyz.shape[0], 2), np.float32)
        if self.prev is not None and self.prev.shape == xyz.shape and now > self.prev_t:
            vel = (xyz[:, :2] - self.prev[:, :2]) / (now - self.prev_t)
        if self.held is not None and self.held.shape == vel.shape:
            vel = p.smooth * self.held + (1.0 - p.smooth) * vel
        self.prev, self.prev_t, self.held = xyz, now, vel
        return vel

    def build(self, p, want):
        """The task `mode` names, on the model it needs, held until a param it was built on moves."""
        self.close()
        self.built = want
        task = TASKS[p.mode]
        opts = {name: getattr(p, feed) for name, feed in task.knobs.items()}
        opts["base_options"] = BaseOptions(model_asset_path=str(fetch(task.model)))
        opts["running_mode"] = vision.RunningMode.VIDEO
        self.task = getattr(vision, task.cls).create_from_options(getattr(vision, f"{task.cls}Options")(**opts))
        self.prev, self.held = None, None

    def close(self):
        if self.task is not None:
            self.task.close()
            self.task = None

    def stop(self):
        self.close()


def fetch(model):
    """The task file, from goofi's home; the network is asked once, the first time a mode runs."""
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
        raise ValueError(f"PoseEstimation reads a picture; this frame is {x.shape}")
    x = x[..., :3]
    # A float frame is the [0, 1] every other image node in goofi carries; an integer one already
    # is what MediaPipe wants, so only the first is scaled.
    if np.issubdtype(x.dtype, np.floating):
        x = np.clip(x, 0.0, 1.0) * 255.0
    return np.ascontiguousarray(x, dtype=np.uint8)


def read(found, mode, width, height):
    """Every point the result carries: `(x, y, z)` rows, and the name each row goes by."""
    if mode == "facebox":
        return spots(found, width, height)
    rows, labels = [], []
    for attr in TASKS[mode].parts:
        for k, group in enumerate(grouped(getattr(found, attr, None))):
            for i, lm in enumerate(group):
                rows.append((lm.x, lm.y, getattr(lm, "z", 0.0)))
                labels.append(f"{attr.removesuffix('_landmarks')}{k}.{i}")
    return rows, labels


def grouped(found):
    """One list per detection. Holistic answers one person as a flat list; the rest nest."""
    if not found:
        return []
    return found if isinstance(found[0], (list, tuple)) else [found]


def spots(found, width, height):
    """A face detection's six keypoints and its box centre, in the picture's own box.

    The keypoints arrive normalized and the box in pixels, so only the box is divided down.
    """
    rows, labels = [], []
    for k, det in enumerate(found.detections):
        for i, point in enumerate(det.keypoints):
            rows.append((point.x, point.y, 0.0))
            labels.append(f"face{k}.{i}")
        box = det.bounding_box
        rows.append(((box.origin_x + box.width * 0.5) / width, (box.origin_y + box.height * 0.5) / height, 0.0))
        labels.append(f"face{k}.centre")
    return rows, labels


def naming(found):
    """What the recogniser called each hand, in the order the hands came out."""
    return ",".join(g[0].category_name for g in found.gestures if g)
