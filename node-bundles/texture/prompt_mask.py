"""PromptMask cuts out what a few words name, as a mask any node can read.

The prompt models in a node of their own, so the nodes that shape and use masks stay light. By
sam 2, Grounding DINO (`IDEA-Research/grounding-dino-tiny`) finds a box for each thing named and
SAM 2.1 (`facebook/sam2.1-hiera-small`) cuts each box out sharply, as latent-lab's "object
(words)" source does. By clipseg, CLIPSeg (`CIDAS/clipseg-rd64-refined`) gives a soft map that
also knows surfaces and moods ("wet", "rough") as well as things.

Outputs
- mask. 0..1 at the picture's size, its edge at 0.5. SAM 2's cut-out as it is, or CLIPSeg's map
  with `confidence` set at 0.5. Into `AdaptiveMask.region` it is reshaped, grown, combined and
  faded like any region. It goes straight to `TextureTransfer.mask`, or through
  `graphics:SignalIn` to `graphics:Composite.mask`.
- field. The model's own map: SAM 2's probability or CLIPSeg's sureness, for
  `AdaptiveMask.field`, where `coverage` takes a share of it.
- preview and status. The status carries the detector's score for each thing found.

Grounding DINO scores a thing the picture lacks about as high as one it holds (0.3 to 0.67 for "a
car" on archive photographs, against 0.33 to 0.44 for real leaves), so a prompt for something absent
cuts out the nearest object. On a live picture the detector looks again every `redetect` seconds,
and SAM 2 cuts each frame in between from the last boxes.
"""

import hashlib
import time

import numpy as np
import torch
import torch.nn.functional as F

import goofi

CLIPSEG = "CIDAS/clipseg-rd64-refined"
SAM = "facebook/sam2.1-hiera-small"
DETECTOR = "IDEA-Research/grounding-dino-tiny"
READERS = ["sam 2", "clipseg"]
WORK = [0, 256, 384, 512, 768, 1024]
GOLD = (0.79, 0.64, 0.33)
LUM = (0.299, 0.587, 0.114)
RESEND = 1.0  # seconds: an unchanged mask is sent again this often, so a viewer opened later sees it


def device():
    """CUDA when the runtime's torch has it, else four CPU threads."""
    if torch.cuda.is_available():
        return "cuda"
    if torch.get_num_threads() > 4:
        torch.set_num_threads(4)
    return "cpu"


def resize(a, h, w, mode="bilinear"):
    """A field [h, w] or a picture [h, w, c] at another size."""
    picture = a.ndim == 3
    t = a.permute(2, 0, 1)[None] if picture else a[None, None]
    if t.shape[-2:] == (h, w):
        return a
    shrink = h < t.shape[-2] or w < t.shape[-1]
    t = F.interpolate(t, size=(h, w), mode=mode, align_corners=False, antialias=shrink)
    return t[0].permute(1, 2, 0) if picture else t[0, 0]


def pretrained(cls, name):
    """A model or processor from the Hugging Face cache, fetched once when it is not there."""
    try:
        return cls.from_pretrained(name, local_files_only=True)
    except OSError:
        return cls.from_pretrained(name)


class Prompter:
    """CLIPSeg's answer to a few words: how sure it is, per place, that the place is what they name.
    Loaded on first use and kept for the last few pictures and prompts."""

    def __init__(self):
        self.model = None
        self.processor = None
        self.kept = {}

    def load(self, dev):
        if self.model is None:
            from transformers import CLIPSegForImageSegmentation, CLIPSegProcessor

            self.processor = pretrained(CLIPSegProcessor, CLIPSEG)
            self.model = pretrained(CLIPSegForImageSegmentation, CLIPSEG).to(dev).eval()
        return self.model

    @torch.inference_mode()
    def __call__(self, key, img, words):
        """[h', w'] in 0..1 (CLIPSeg's own grid), the most sure of the prompts at each place."""
        if key in self.kept:
            return self.kept[key]
        model = self.load(img.device)
        rgb = (img.clamp(0, 1) * 255).round().byte().cpu().numpy()
        x = self.processor(text=words, images=[rgb] * len(words), padding=True, return_tensors="pt")
        logits = model(**{k: v.to(img.device) for k, v in x.items()}).logits
        sure = torch.sigmoid(logits.reshape(len(words), *logits.shape[-2:]).float()).amax(0)
        if len(self.kept) > 4:
            self.kept.pop(next(iter(self.kept)))
        self.kept[key] = sure
        return sure


class Cutter:
    """What a few words name, cut out: Grounding DINO finds a box for each thing the words name,
    SAM 2.1 cuts each box out, and the cut-outs are taken together. Loaded on first use and kept
    for the last few pictures and prompts."""

    def __init__(self):
        self.models = {}
        self.kept = {}
        self.seen = None  # the picture SAM 2's encoding below is of
        self.encoding = None
        self.text = None  # the prompt as the detector reads it, kept while the words stay the same
        self.boxes = None  # (what was asked, when, boxes, scores): the last detection

    def load(self, name, dev):
        if name not in self.models:
            if name == "sam":
                # The model alone: SAM 2's processor in transformers needs torchvision, which goofi's
                # runtime does not carry, and what it does is the few lines in __call__.
                from transformers import Sam2Model

                processor, model = None, pretrained(Sam2Model, SAM)
            else:
                from transformers import AutoModelForZeroShotObjectDetection, AutoProcessor

                processor = pretrained(AutoProcessor, DETECTOR)
                model = pretrained(AutoModelForZeroShotObjectDetection, DETECTOR)
            self.models[name] = (processor, model.to(dev).eval())
        return self.models[name]

    def detect(self, img, words, sure, at):
        """Grounding DINO's boxes for the words, in the picture's pixels, and their scores. The picture
        is prepared as the detector's processor prepares it (its short side at `at`, the long side at
        most 1333 / 800 of that, normalised as ImageNet is), on the picture's own device."""
        processor, detector = self.load("detector", img.device)
        if self.text is None or self.text[0] != tuple(words):
            t = processor(text=[[s.lower() for s in words]], return_tensors="pt")
            self.text = (tuple(words), {k: v.to(img.device) for k, v in t.items()})
        h, w = img.shape[:2]
        k = min(at / min(h, w), at * 1333 / 800 / max(h, w))
        x = resize(img.clamp(0, 1), max(1, round(h * k)), max(1, round(w * k))).permute(2, 0, 1)[None]
        mean = torch.tensor((0.485, 0.456, 0.406), device=img.device).view(1, 3, 1, 1)
        std = torch.tensor((0.229, 0.224, 0.225), device=img.device).view(1, 3, 1, 1)
        mask = torch.ones(1, x.shape[2], x.shape[3], dtype=torch.long, device=img.device)
        out = detector(pixel_values=(x - mean) / std, pixel_mask=mask, **self.text[1])
        found = processor.post_process_grounded_object_detection(
            out, self.text[1]["input_ids"], threshold=sure, text_threshold=min(sure, 0.25), target_sizes=[(h, w)])[0]
        return found["boxes"].cpu().tolist(), [round(float(s), 2) for s in found["scores"].cpu().tolist()]

    @torch.inference_mode()
    def __call__(self, key, img, words, sure, at=800, every=0.0):
        """[h, w] in 0..1, every thing found cut out, and the detector's score for each. Within `every`
        seconds of the last detection for the same words, a new picture is cut from the last boxes,
        as a live picture moves little from one frame to the next. SAM 2 alone costs a fraction of the
        detector."""
        if key in self.kept:
            return self.kept[key]
        h, w = img.shape[:2]
        asked = (tuple(words), sure, at, h, w)
        now = time.monotonic()
        if self.boxes is not None and self.boxes[0] == asked and now - self.boxes[1] < every:
            boxes, scores = self.boxes[2], self.boxes[3]
        else:
            boxes, scores = self.detect(img, words, sure, at)
            self.boxes = (asked, now, boxes, scores)
        if boxes:
            _, sam = self.load("sam", img.device)
            # As SAM 2's processor prepares them: the picture squeezed to 1024 x 1024 and normalised
            # as ImageNet is, the boxes carried into that square, the masks brought back bilinear.
            side = 1024
            x = resize(img.clamp(0, 1), side, side).permute(2, 0, 1)[None]
            mean = torch.tensor((0.485, 0.456, 0.406), device=img.device).view(1, 3, 1, 1)
            std = torch.tensor((0.229, 0.224, 0.225), device=img.device).view(1, 3, 1, 1)
            b = torch.tensor(boxes, dtype=torch.float32, device=img.device)
            b = b * torch.tensor([side / w, side / h, side / w, side / h], device=img.device)
            # SAM 2 sees the picture once: another prompt on the same picture costs only its decoder.
            if self.seen != (key[0], h, w):
                self.encoding = sam.get_image_embeddings((x - mean) / std)
                self.seen = (key[0], h, w)
            out = sam(image_embeddings=self.encoding, input_boxes=b[None], multimask_output=False)
            logits = out.pred_masks[0, :, 0].float()  # [things, 256, 256]
            logits = F.interpolate(logits[None], size=(h, w), mode="bilinear", align_corners=False)[0]
            cut = torch.sigmoid(logits).amax(0)
        else:
            cut = torch.zeros(h, w, device=img.device)
        if len(self.kept) > 4:
            self.kept.pop(next(iter(self.kept)))
        self.kept[key] = (cut, scores)
        return self.kept[key]


def words_of(text):
    return [w.strip() for w in str(text).split(",") if w.strip()]


def picture(value, name):
    """Whatever came down the wire, as float RGB in 0..1 [h, w, 3]: a stack [n, h, w, c] gives its
    first picture."""
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


def fingerprint(x):
    a = np.ascontiguousarray(x).reshape(-1)
    step = max(1, a.size // 65536)
    return x.shape, hashlib.blake2b(np.ascontiguousarray(a[::step]).tobytes(), digest_size=12).hexdigest()


def clamp(v, lo, hi):
    return min(max(float(v), lo), hi)


class PromptMask(goofi.Node):
    """Cut out what a few words name: SAM 2 for things with sharp edges, CLIPSeg for soft surfaces."""

    TAGS = ["image", "analysis", "ml"]
    INPUTS = {"image": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {"mask": goofi.DataType.ARRAY, "field": goofi.DataType.ARRAY, "preview": goofi.DataType.ARRAY,
               "status": goofi.DataType.STRING}
    PARAMS = {
        "prompt": {
            "prompt": goofi.StringParam("flower", doc="What to cut out, in a few words. Commas separate things "
                                        "taken together, as in `moss, lichen`."),
            "by": goofi.StringParam("sam 2", READERS, doc="Who reads the prompt. sam 2 cuts out each thing named with "
                                    "a sharp edge (Grounding DINO and SAM 2.1, fetched once, 0.9 GB). clipseg gives a "
                                    "soft map that also knows surfaces such as wet or rough (0.6 GB)."),
            "confidence": goofi.NumParam(0.3, 0.0, 1.0, doc="How sure the model must be that a thing (with sam 2) or a "
                                         "place (with clipseg) is what the prompt names. Lower takes more."),
            "detect_at": goofi.NumParam(800, 384, 800, int=True, options=[384, 512, 800], doc="Pixels on the short side "
                                        "Grounding DINO reads the picture at. 800 is its own. 384 is faster and found "
                                        "the same things in tests.", show=("by", ["sam 2"])),
            "redetect": goofi.NumParam(0.5, 0.0, 5.0, doc="Seconds between Grounding DINO's looks on a live picture. "
                                       "In between, SAM 2 cuts each new frame from the last boxes. 0 looks every frame.",
                                       show=("by", ["sam 2"])),
        },
        "frame": {
            "work": goofi.NumParam(512, 0, 1024, int=True, options=WORK, doc="Pixels on the long side the picture is "
                                   "read at, 0 for its own. The mask comes out at the picture's size."),
        },
        "common": {"autotrigger": goofi.BoolParam(True), "max_frequency": goofi.NumParam(30.0, 1.0, 120.0)},
    }

    def setup(self):
        self.prompter = Prompter()
        self.cutter = Cutter()
        self.key = None
        self.result = None
        self.sent = 0.0

    def process(self, image):
        if image is None:
            return None
        p = self.params
        frame = picture(image.data, "image")
        key = (fingerprint(frame), repr(vars(p.prompt)), repr(vars(p.frame)))
        if key == self.key:
            if self.result is not None and time.monotonic() - self.sent >= RESEND:
                self.sent = time.monotonic()
                return self.result
            return None
        self.key = key
        started = time.perf_counter()
        dev = device()
        words = words_of(p.prompt.prompt)
        sure = clamp(p.prompt.confidence, 0.0, 1.0)
        with torch.inference_mode():
            src = torch.from_numpy(frame).to(dev)
            H, W = src.shape[:2]
            long = int(p.frame.work) or max(H, W)
            k = min(1.0, long / max(H, W))
            h, w = max(8, round(H * k)), max(8, round(W * k))
            img = resize(src, h, w)
            found = None
            if not words:
                field = mask = torch.zeros(h, w, device=dev)
            elif p.prompt.by == "sam 2":
                at = int(p.prompt.detect_at)
                field, found = self.cutter((key[0], tuple(words), round(sure, 3), at), img, words, max(sure, 0.05), at,
                                           clamp(p.prompt.redetect, 0.0, 60.0))
                mask = field
            elif p.prompt.by == "clipseg":
                field = resize(self.prompter((key[0], tuple(words)), img, words), h, w)
                x = ((field - (sure - 0.1)) / 0.2).clamp(0.0, 1.0)  # the edge at `confidence`, 0.2 wide
                mask = x * x * (3.0 - 2.0 * x)
            else:
                raise ValueError(f"prompt/by must be one of {', '.join(READERS)}")
            mask = resize(mask, H, W).clamp(0.0, 1.0)
            field = resize(field, H, W).clamp(0.0, 1.0)
            grey = (src @ torch.tensor(LUM, device=dev) * 0.55)[..., None]
            shown = grey * (1.0 - mask[..., None]) + (grey * 0.45 + torch.tensor(GOLD, device=dev) * 0.75) * mask[..., None]
            out_mask = np.ascontiguousarray(mask.float().cpu().numpy())
            out_field = np.ascontiguousarray(field.float().cpu().numpy())
            preview = np.ascontiguousarray(shown.clamp(0, 1).float().cpu().numpy())
        ms = (time.perf_counter() - started) * 1000.0
        what = f"{', '.join(words) or 'no words yet'} by {p.prompt.by}"
        if found is not None:
            what += f", {len(found)} found" + (f" (scores {', '.join(map(str, found))})" if found else "")
        covered = float((out_mask > 0.5).mean())
        self.result = {"mask": (out_mask, {"prompt": {"words": words, "by": p.prompt.by, "scores": found or []}}),
                       "field": (out_field, {"prompt": {"words": words, "by": p.prompt.by}}),
                       "preview": (preview, {"channels": {"dim2": ["r", "g", "b"]}}),
                       "status": f"{what} | {covered:.0%} covered | {w} x {h} | {ms:.0f} ms on {dev}"}
        self.sent = time.monotonic()
        return self.result


# ---- the shared worker ------------------------------------------------------------------------
# `python prompt_mask.py --serve` holds the prompt models for every AdaptiveMask in a patch. goofi
# runs AdaptiveMask inside its own free-threaded Python, which has no torch, so the masks that are
# given a prompt send their pictures here, one process for all of them, started by the first.

IDLE = 600.0  # seconds without a request before the worker leaves, giving its memory back


def cut_for(cutter, prompter, key, img, words, model, sure, at, every):
    """The cut-out [h, w] in 0..1 with its edge at 0.5, and Grounding DINO's scores with sam 2."""
    dev = device()
    h, w = img.shape[:2]
    with torch.inference_mode():
        t = torch.from_numpy(np.ascontiguousarray(img, dtype=np.float32)).to(dev)
        if model == "sam 2":
            cut, found = cutter((key, tuple(words), round(sure, 3), at), t, words, max(sure, 0.05), at, every)
            return cut.float().cpu().numpy(), found
        if model != "clipseg":
            raise ValueError(f"the model must be one of {', '.join(READERS)}")
        sure_map = resize(prompter((key, tuple(words)), t, words), h, w)
        x = ((sure_map - (sure - 0.1)) / 0.2).clamp(0.0, 1.0)  # the edge at `confidence`, 0.2 wide
        return (x * x * (3.0 - 2.0 * x)).float().cpu().numpy(), None


def serve():
    """Answer requests from AdaptiveMask nodes until the pipe closes or IDLE passes without one.
    Each message is a length (8 bytes) and a pickle, both ways. Anything printed goes to stderr."""
    import os
    import pickle
    import queue
    import struct
    import sys
    import threading

    # The pipes move to private handles and the standard ones point elsewhere. A library's print must
    # not land in the replies. And on Windows a thread waiting to read a synchronous pipe holds every
    # other call on it, so an import that asks whether stdin is a terminal would wait for the next
    # request, which never comes while the mask waits for this load.
    reply_to = os.fdopen(os.dup(1), "wb")
    requests = os.fdopen(os.dup(0), "rb")
    os.dup2(2, 1)
    nothing = os.open(os.devnull, os.O_RDONLY)
    os.dup2(nothing, 0)
    os.close(nothing)
    sys.stdin = open(os.devnull)
    asks = queue.Queue()

    began = time.perf_counter()

    def say(text):
        print(f"prompt worker {os.getpid()} at {time.perf_counter() - began:6.1f} s: {text}", file=sys.stderr, flush=True)

    def listen():
        stream = requests
        while True:
            head = stream.read(8)
            if len(head) < 8:  # goofi closed the pipe, so nobody is left to answer, even mid-load
                say("the pipe closed, leaving")
                os._exit(0)
            asks.put(pickle.loads(stream.read(struct.unpack("<Q", head)[0])))

    threading.Thread(target=listen, daemon=True).start()
    say(f"started on {device()}")
    cutter, prompter = Cutter(), Prompter()
    while True:
        try:
            ask = asks.get(timeout=IDLE)
        except queue.Empty:
            say("idle, leaving")
            return
        try:
            if ask["op"] == "warm":
                say(f"loading {ask['model']}")
                if ask["model"] == "sam 2":
                    cutter.load("detector", device())
                    cutter.load("sam", device())
                else:
                    prompter.load(device())
                say(f"{ask['model']} ready")
                answer = {"ok": True, "device": device()}
            else:
                cut, found = cut_for(cutter, prompter, ask["key"], ask["img"], ask["words"], ask["model"],
                                     ask["sure"], ask["at"], ask["every"])
                answer = {"ok": True, "cut": cut, "found": found}
        except Exception as e:
            answer = {"ok": False, "error": f"{type(e).__name__}: {e}"}
            say(answer["error"])
        data = pickle.dumps(answer, protocol=5)
        reply_to.write(struct.pack("<Q", len(data)) + data)
        reply_to.flush()


if __name__ == "__main__" and "--serve" in __import__("sys").argv:
    serve()
