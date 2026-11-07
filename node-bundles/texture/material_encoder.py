"""MaterialEncoder places a picture in a material space, and builds spaces from folders of photographs.

The image encoder the IP-Adapter was trained with (CLIP ViT-H, `h94/IP-Adapter`,
`models/image_encoder`), in a node of its own so MaterialSpace stays a light lookup.

- place. A wired picture's place in the space: the centre square of the picture, read as one
  1024-number vector. Wire it into `MaterialSpace.place` and the space hands on the photographs
  nearest whatever the camera sees.
- build. `folder` and the `build` pulse cut every photograph under the folder (subfolders are
  categories) into `crops` crops, place each crop, and keep the space where MaterialSpace finds it
  by the same folder, in goofi's plugin data. The file is a latent-lab space (`ip_space.py`), and
  the build runs in the background with its progress on the status.
"""

import hashlib
import os
import pathlib
import random
import threading
import time

import numpy as np
import torch

import goofi

ENCODER = ("h94/IP-Adapter", "models/image_encoder")  # the sd15 adapter's ViT-H, which the lab's spaces use
CLIP_MEAN = (0.48145466, 0.4578275, 0.40821073)
CLIP_STD = (0.26862954, 0.26130258, 0.27577711)
SUFFIXES = {".png", ".jpg", ".jpeg", ".bmp", ".webp", ".tif", ".tiff"}


def data_dir():
    """goofi's persistent data for this bundle, where MaterialSpace looks for built spaces."""
    home = pathlib.Path(os.environ.get("GOOFI_HOME") or pathlib.Path.home()) / ".goofi"
    return home / "plugin-data" / "texture" / "spaces"


def built_path(folder):
    """Where the space built from `folder` is kept (MaterialSpace reads it by the same rule)."""
    folder = pathlib.Path(folder).expanduser().resolve()
    tag = hashlib.blake2b(str(folder).lower().encode(), digest_size=6).hexdigest()
    return data_dir() / f"{folder.name}-{tag}.npz"


def images_in(folder):
    return sorted(p for p in pathlib.Path(folder).expanduser().rglob("*") if p.suffix.lower() in SUFFIXES)


class Encoder:
    """The IP-Adapter's image encoder, loaded on first use."""

    def __init__(self):
        self.model = None
        self.lock = threading.Lock()

    def load(self):
        if self.model is None:
            from transformers import CLIPVisionModelWithProjection

            dev = "cuda" if torch.cuda.is_available() else "cpu"
            if dev == "cpu" and torch.get_num_threads() > 8:
                torch.set_num_threads(8)  # the rest of the patch keeps running while a space builds
            try:
                m = CLIPVisionModelWithProjection.from_pretrained(ENCODER[0], subfolder=ENCODER[1], local_files_only=True)
            except OSError:
                m = CLIPVisionModelWithProjection.from_pretrained(ENCODER[0], subfolder=ENCODER[1])
            self.model = m.to(dev, torch.float16 if dev == "cuda" else torch.float32).eval()
            self.dev = dev
        return self.model

    def __call__(self, crops):
        """[n, 224, 224, 3] in 0..1 -> [n, 1024] image embeddings."""
        with self.lock:  # the build's thread and the live picture share one model
            model = self.load()
            x = torch.as_tensor(np.asarray(crops, np.float32), device=self.dev).permute(0, 3, 1, 2)
            mean = torch.tensor(CLIP_MEAN, device=self.dev).view(1, 3, 1, 1)
            std = torch.tensor(CLIP_STD, device=self.dev).view(1, 3, 1, 1)
            with torch.inference_mode():
                out = model(pixel_values=((x - mean) / std).to(model.dtype)).image_embeds
            return out.float().cpu().numpy()


def crop(path, side, detail, rng):
    """One crop as the lab cuts them: square, a `detail`-th of the frame's short side, flipped at random."""
    from PIL import Image

    big = max(side, round(side * detail))
    with Image.open(path) as im:
        shrink = min(im.size) / big
        if shrink >= 2:
            im.draft("RGB", (round(im.width / shrink), round(im.height / shrink)))
        im = im.convert("RGB")
    k = big / min(im.size)
    im = im.resize((max(big, round(im.width * k)), max(big, round(im.height * k))), Image.LANCZOS)
    left, top = rng.randint(0, im.width - side), rng.randint(0, im.height - side)
    im = im.crop((left, top, left + side, top + side))
    if rng.random() < 0.5:
        im = im.transpose(Image.FLIP_LEFT_RIGHT)
    if rng.random() < 0.5:
        im = im.transpose(Image.FLIP_TOP_BOTTOM)
    return np.asarray(im, np.float32) / 255.0


def build(folder, out, crops, detail, encoder, report):
    """Embed `crops` crops of every image under `folder` and write a lab space to `out`."""
    files = images_in(folder)
    if not files:
        raise ValueError(f"no images under {folder}")
    names = sorted({f.parent.name for f in files})
    rng = random.Random(0)
    vecs, owner, tag = [], [], []
    for i, f in enumerate(files):
        try:
            batch = [crop(f, 224, detail, rng) for _ in range(crops)]
        except OSError:
            continue
        vecs.append(encoder(batch))
        owner += [i] * len(batch)
        tag += [names.index(f.parent.name)] * len(batch)
        report(f"building: {i + 1} / {len(files)} photographs")
    emb = np.concatenate(vecs).astype(np.float32)
    owner, tag = np.array(owner), np.array(tag)
    centre = emb.mean(0)
    _, s, vt = np.linalg.svd(emb - centre, full_matrices=False)
    keep = min(32, len(s))
    means = np.stack([emb[tag == c].mean(0) for c in range(len(names))])
    out.parent.mkdir(parents=True, exist_ok=True)
    part = out.with_name(out.stem + f".{os.getpid()}.part.npz")
    np.savez(part, emb=emb, owner=owner, tag=tag, files=np.array([str(f) for f in files]), names=np.array(names),
             centre=centre.astype(np.float32), axes=vt[:keep].astype(np.float32),
             spread=(s[:keep] / np.sqrt(max(len(emb) - 1, 1))).astype(np.float32), means=means.astype(np.float32),
             norm=np.float32(np.median(np.linalg.norm(emb, axis=1))), preset=np.array("sd15"))
    part.replace(out)


def as_picture(value, name):
    """A picture [h, w, 3] in 0..1 from whatever came down the wire: grey, RGBA, 0..255, or the first
    of a stack."""
    x = np.asarray(value)
    if x.ndim == 4:
        x = x[0]
    if x.ndim == 2:
        x = np.repeat(x[..., None], 3, axis=-1)
    if x.ndim != 3 or x.shape[2] < 3:
        raise ValueError(f"`{name}` takes a picture [height, width, 3]. This frame is {list(x.shape)}")
    x = x[..., :3]
    if not np.issubdtype(x.dtype, np.floating):
        return x.astype(np.float32) / 255.0
    return np.clip(np.nan_to_num(x.astype(np.float32, copy=False)), 0.0, 1.0)


class MaterialEncoder(goofi.Node):
    """Place a picture in a material space as the IP-Adapter's eye sees it, or build a space from a folder."""

    TAGS = ["image", "ml", "analysis"]
    INPUTS = {"picture": goofi.InputSlot(goofi.DataType.ARRAY)}
    OUTPUTS = {"place": goofi.DataType.ARRAY, "status": goofi.DataType.STRING}
    PARAMS = {
        "build": {
            "folder": goofi.StringParam("", doc="A folder of photographs (subfolders are categories) to build a space "
                                        "from. MaterialSpace finds the space by the same folder."),
            "build": goofi.PulseParam(doc="Embed the folder's photographs and keep the space. It runs in the background."),
            "crops": goofi.NumParam(4, 1, 16, int=True, doc="Crops embedded per photograph."),
            "detail": goofi.NumParam(3.0, 1.0, 8.0, doc="How close a crop looks. At 3 it sees a third of the "
                                     "photograph's short side."),
        },
        "common": {"autotrigger": goofi.BoolParam(True), "max_frequency": goofi.NumParam(10.0, 0.5, 60.0)},
    }

    def setup(self):
        self.encoder = Encoder()
        self.job = None
        self.note = ""
        self.key = None
        self.result = None
        self.sent = 0.0

    def pulse_build_build(self):
        folder = self.params.build.folder.strip()
        if not folder or not pathlib.Path(folder).is_dir():
            raise ValueError("Set build/folder to a folder of photographs before building")
        if self.job is not None and self.job.is_alive():
            raise ValueError("A build is running already")
        out = built_path(folder)
        crops, detail = int(self.params.build.crops), float(self.params.build.detail)

        def run():
            try:
                build(folder, out, crops, detail, self.encoder, self.report)
                self.note = f"built {out.name}"
            except Exception as e:  # reported on the status output, and the node carries on
                self.note = f"build failed: {e}"

        self.note = "building: starting"
        self.job = threading.Thread(target=run, daemon=True)
        self.job.start()

    def report(self, text):
        self.note = text

    def process(self, picture=None):
        status = self.note or "Wire a picture to place it, or set build/folder and press build"
        if picture is None:
            return {"status": status}
        q = as_picture(picture.data, "picture")
        key = (q.shape, hashlib.blake2b(np.ascontiguousarray(q).tobytes()[::97]).hexdigest())
        if key == self.key:
            if self.result is not None and self.result["status"].split(" | ", 1)[-1] != status:
                self.result["status"] = self.result["status"].split(" | ")[0] + f" | {status}"
                return {"status": self.result["status"]}
            if self.result is not None and time.monotonic() - self.sent >= 1.0:
                self.sent = time.monotonic()
                return self.result
            return None
        self.key = key
        from PIL import Image

        started = time.perf_counter()
        side = min(q.shape[:2])
        top, left = (q.shape[0] - side) // 2, (q.shape[1] - side) // 2
        sq = Image.fromarray((q[top:top + side, left:left + side] * 255).astype(np.uint8)).resize((224, 224))
        place = self.encoder([np.asarray(sq, np.float32) / 255.0])[0].astype(np.float32)
        ms = (time.perf_counter() - started) * 1000.0
        self.result = {"place": (np.ascontiguousarray(place), {"space": {"encoder": "/".join(ENCODER)}}),
                       "status": f"placed in {ms:.0f} ms on {self.encoder.dev} | {status}"}
        self.sent = time.monotonic()
        return self.result
