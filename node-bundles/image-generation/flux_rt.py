"""Edit image streams with a persistent, optional FluxRT runtime."""
import json
import math
import os
from collections import deque
from pathlib import Path
import threading
import time
from typing import NamedTuple

import cv2
import numpy as np
from PIL import Image
from fluxrt.stream_processor.model_inference_subprocess import ModelInferenceSubprocess
import goofi


def available_commit():
    """Read Windows commit headroom, including the paging file."""
    if os.name != 'nt':
        return None
    import ctypes
    class MemoryStatus(ctypes.Structure):
        _fields_ = [('length', ctypes.c_uint32), ('load', ctypes.c_uint32)] + [
            (name, ctypes.c_uint64) for name in (
                'total_physical', 'available_physical', 'total_page_file',
                'available_page_file', 'total_virtual', 'available_virtual', 'extended')]
    status = MemoryStatus()
    status.length = ctypes.sizeof(status)
    if not ctypes.windll.kernel32.VariableMemoryStatusEx(ctypes.byref(status)):
        raise OSError('Cannot check Windows memory before loading FluxRT')
    return status.available_page_file


def process_commit():
    """Read memory already committed by this worker, including runtime libraries."""
    if os.name != 'nt':
        return 0
    import ctypes
    class Counters(ctypes.Structure):
        _fields_ = [('size', ctypes.c_uint32), ('faults', ctypes.c_uint32)] + [
            (name, ctypes.c_size_t) for name in (
                'peak_working', 'working', 'peak_paged', 'paged', 'peak_nonpaged',
                'nonpaged', 'page_file', 'peak_page_file', 'private')]
    counters = Counters()
    counters.size = ctypes.sizeof(counters)
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.GetCurrentProcess.restype = ctypes.c_void_p
    read = ctypes.WinDLL('psapi', use_last_error=True).GetProcessMemoryInfo
    read.argtypes = [ctypes.c_void_p, ctypes.POINTER(Counters), ctypes.c_uint32]
    if not read(kernel.GetCurrentProcess(), ctypes.byref(counters), counters.size):
        raise ctypes.WinError(ctypes.get_last_error())
    return counters.private


class Controls(NamedTuple):
    steps: int
    guidance: float
    source: float
    style: float
    width: int
    height: int


class Settings(NamedTuple):
    prompt: str
    seed: int
    reference: np.ndarray | None
    transition: str
    duration: float
    frames: int
    controls: Controls
    consistency: float
    align_motion: bool
    reset: int
    prompt_b: str
    mix: float
    smooth: float
    reference_enabled: bool
    reference_role: str
    cut: int
    output_fps: float


class Feedback:
    """Retain only the source and generated image from the last inference."""
    def __init__(self):
        self.source = self.output = None

    def clear(self):
        self.source = self.output = None

    def prepare(self, source, strength, align_motion):
        if not strength or self.source is None:
            return source, 0.0
        previous = self.output
        weight = np.full(source.shape[:2], strength, dtype=np.float32)
        if align_motion:
            h, w = source.shape[:2]
            scale = min(1.0, 256/max(h, w))
            size = (round(w*scale), round(h*scale))
            current = cv2.resize(cv2.cvtColor(source, cv2.COLOR_RGB2GRAY), size)
            old = cv2.resize(cv2.cvtColor(self.source, cv2.COLOR_RGB2GRAY), size)
            backward = cv2.calcOpticalFlowFarneback(current, old, None, .5, 3, 21, 3, 5, 1.2, 0)
            forward = cv2.calcOpticalFlowFarneback(old, current, None, .5, 3, 21, 3, 5, 1.2, 0)
            y, x = np.mgrid[:size[1], :size[0]].astype(np.float32)
            mx, my = x+backward[..., 0], y+backward[..., 1]
            warped_old = cv2.remap(old, mx, my, cv2.INTER_LINEAR)
            returned = cv2.remap(forward, mx, my, cv2.INTER_LINEAR)
            gray = current.astype(np.float32)
            variance = cv2.blur(gray*gray, (7, 7)) - cv2.blur(gray, (7, 7))**2
            valid = ((mx >= 0) & (mx < size[0]-1) & (my >= 0) & (my < size[1]-1)
                     & (np.linalg.norm(backward+returned, axis=2) < 1.5)
                     & (np.abs(warped_old.astype(np.float32)-gray) < 18)
                     & (variance > 4))
            confidence = cv2.GaussianBlur(valid.astype(np.float32), (5, 5), 0)*valid
            weight *= cv2.resize(confidence, (w, h), interpolation=cv2.INTER_NEAREST)
            flow = cv2.resize(backward, (w, h))
            flow[..., 0] *= w/size[0]
            flow[..., 1] *= h/size[1]
            y, x = np.mgrid[:h, :w].astype(np.float32)
            previous = cv2.remap(previous, x+flow[..., 0], y+flow[..., 1], cv2.INTER_LINEAR)
        mixed = source.astype(np.float32)*(1-weight[..., None]) + previous*weight[..., None]
        return np.ascontiguousarray(np.clip(np.rint(mixed), 0, 255), dtype=np.uint8), float(weight.mean()/strength)

    def remember(self, source, output):
        self.source, self.output = source, output


class Inference(ModelInferenceSubprocess):
    """Use the pinned model pipeline inside goofi's existing Python worker."""
    def __init__(self, config):
        self.config = config
        self.resolution = config['resolution']
        self.height, self.width = self.resolution['height'], self.resolution['width']
        self.interpolation_exp = config['interpolation_exp']
        self.logging = False

    def init_shared_tensors(self):
        """The node owns the frame handoff; no additional processes are needed."""

    @staticmethod
    def release():
        import gc
        import torch
        gc.collect()
        torch.cuda.empty_cache()

    def process_init(self):
        super().process_init()
        self.controls = None
        self.negative_prompt_embeds = None
        self.attention_bias = None
        self.reference_primary = False

    def set_reference(self, enabled, primary):
        if enabled != self.config['use_reference_image'] or primary != self.reference_primary:
            self.config['use_reference_image'] = enabled
            self.reference_primary = primary
            self.controls = None

    def noise_for_seed(self, seed):
        """Make the unpacked noise accepted by the pinned pipeline."""
        import torch
        scale = self.pipe.vae_scale_factor * 2
        shape = (1, self.pipe.transformer.config.in_channels,
                 self.height // scale, self.width // scale)
        return torch.randn(shape, generator=torch.Generator(device=self.device).manual_seed(seed),
                           device=self.device, dtype=self.prompt_embeds.dtype)

    @staticmethod
    def blend_noise(source, target, progress):
        """Interpolate noise directions without reducing their magnitude."""
        import torch
        a, b = source.float(), target.float()
        na, nb = a.norm(), b.norm()
        cosine = ((a * b).sum() / (na * nb).clamp_min(1e-12)).clamp(-1, 1)
        angle = torch.acos(cosine)
        if abs(cosine.item()) > 0.9995:
            return torch.lerp(a, b, progress).to(source.dtype)
        direction = (torch.sin((1-progress)*angle)*a/na
                     + torch.sin(progress*angle)*b/nb) / torch.sin(angle)
        return (direction * ((1-progress)*na + progress*nb)).to(source.dtype)

    def configure(self, controls):
        """Apply model controls and invalidate only the affected caches."""
        if (controls.width, controls.height) != (self.width, self.height):
            raise ValueError('Restart FluxRT after changing source/width or source/height')
        if controls == self.controls:
            return
        import torch
        from fluxrt.stream_processor.update_controller import UpdateController
        previous = self.controls
        if previous is None or controls.steps != previous.steps:
            self.pipe._attention_kwargs = None
            self.pipe.spatial_cache.clear()
            self.config['enable_spatial_cache'] = controls.steps <= 4
            self.update_controller = UpdateController(self.config, self.height, self.width,
                compression_ratio=16, reference_image_seq_len=256 if self.config['use_reference_image'] else None)
            self.pipe.update_controller = self.update_controller
        self.process_state['steps'] = controls.steps
        self.attention_bias = None
        if controls.source != 1 or (self.config['use_reference_image'] and controls.style != 1):
            text = self.prompt_embeds.shape[1]
            image = self.width*self.height//256
            reference = 256 if self.config['use_reference_image'] else 0
            self.attention_bias = torch.zeros(1, 1, 1, text+2*image+reference, device=self.device, dtype=self.dtype)
            first = controls.style if self.reference_primary and reference else controls.source
            second = controls.source if self.reference_primary else controls.style
            self.attention_bias[..., text+image:text+2*image] = math.log(first) if first else -torch.inf
            if reference:
                self.attention_bias[..., text+2*image:] = math.log(second) if second else -torch.inf
        if controls.guidance > 1 and self.negative_prompt_embeds is None:
            with torch.no_grad():
                self.negative_prompt_embeds, _ = self.pipe.encode_prompt(prompt='', device=self.device,
                    num_images_per_prompt=1, max_sequence_length=512, text_encoder_out_layers=(9, 18, 27))
        self.update_controller.reset_cache()
        self.controls = controls

    def process_frame_with_pipeline(self, frame):
        """Generate with independent source and reference attention weights."""
        import torch
        images = [Image.fromarray(frame)]
        if self.config['use_reference_image']:
            reference = np.asarray(self.reference_image)
            if self.reference_primary:
                images = [Image.fromarray(resize_image(reference, self.width, self.height)),
                          Image.fromarray(resize_image(frame, 256, 256))]
            else:
                images.append(Image.fromarray(resize_image(reference, 256, 256)))
        result = self.pipe(prompt_embeds=self.prompt_embeds, negative_prompt_embeds=self.negative_prompt_embeds,
            image=images, height=self.height, width=self.width, guidance_scale=self.controls.guidance,
            num_inference_steps=self.controls.steps,
            latents=self.noise.clone(),
            generator=torch.Generator(device=self.device).manual_seed(self.process_state['seed']),
            attention_kwargs={'attention_mask': self.attention_bias} if self.attention_bias is not None else None,
            output_type='np').images[0]
        return (np.clip(result, 0, 1)*255).astype(np.uint8)


class Runtime:
    """One model owner and a bounded handoff to one interpolation clock."""
    def __init__(self, config, image, settings):
        self.cv = threading.Condition()
        self.closed = False
        self.paused = False
        self.image, self.settings = image, settings
        self.pending = None
        self.output = None
        self.error = None
        self.status = 'Loading'
        self.config = config
        self.infer = threading.Thread(target=self.generate, daemon=True, name='fluxrt-inference')
        self.present = threading.Thread(target=self.play, daemon=True, name='fluxrt-output')
        self.infer.start()
        self.present.start()

    def submit(self, image, settings):
        with self.cv:
            if image is not None:
                self.image = image
            self.settings = settings

    def take(self):
        with self.cv:
            output, self.output = self.output, None
            return output, self.status, self.error

    def pause(self, paused):
        with self.cv:
            self.paused = paused
            if paused:
                self.pending = self.output = None
            self.cv.notify_all()

    def request_stop(self):
        with self.cv:
            self.closed = True
            self.pending = self.output = None
            self.cv.notify_all()

    def generate(self):
        try:
            with self.cv:
                while not self.closed:
                    available = available_commit()
                    required = max(2.0, self.config['memory_budget'] - process_commit()/1024**3)
                    if not self.paused and (available is None or available >= required * 1024**3):
                        break
                    self.status = ('Paused — model not loaded' if self.paused else
                                   f'Waiting for memory — {available/1024**3:.1f} GiB available; '
                                   f'{required:.1f} GiB additional headroom estimated. Retrying automatically.')
                    self.cv.wait(timeout=1.0)
                if self.closed:
                    return
                self.status = 'Loading'
            model = Inference(self.config)
            model.process_init()
            applied = self.settings._replace(prompt=self.config['default_prompt'], prompt_b='',
                seed=self.config['default_seed'], reference=None)
            feedback = Feedback()
            current = target = model.prompt_embeds
            embeds_a = embeds_b = current
            smoothed = None
            has_generated = False
            last_control_time = time.monotonic()
            noise = target_noise = model.noise_for_seed(applied.seed)
            noise_from = None
            current_style = target_style = style_from = None
            blend_from = None
            blend_start = 0.0
            progress = 1.0
            while True:
                with self.cv:
                    if self.paused and not self.closed:
                        pause_start = time.monotonic()
                        self.cv.wait_for(lambda: self.closed or not self.paused)
                        blend_start += time.monotonic() - pause_start
                        last_control_time = time.monotonic()
                    if self.closed:
                        return
                    image, settings = self.image, self.settings
                start = time.monotonic()
                force_cut = settings.cut != applied.cut
                goals = np.array([settings.controls.guidance, settings.controls.source,
                                  settings.controls.style if settings.reference_enabled else 0.0,
                                  settings.mix, settings.consistency], dtype=np.float64)
                previous_mix = None if smoothed is None else float(smoothed[3])
                previous_consistency = 0.0 if smoothed is None else float(smoothed[4])
                alpha = 1.0 if settings.smooth == 0 or force_cut else 1-math.exp(-3*(start-last_control_time)/settings.smooth)
                smoothed = goals.copy() if smoothed is None else smoothed+(goals-smoothed)*alpha
                smoothed[np.abs(goals-smoothed) < 1e-4] = goals[np.abs(goals-smoothed) < 1e-4]
                last_control_time = start
                controls = settings.controls._replace(guidance=float(smoothed[0]), source=float(smoothed[1]), style=float(smoothed[2]))
                active_reference = settings.reference is not None and (settings.reference_enabled or controls.style > 0)
                model.set_reference(active_reference, active_reference and settings.reference_role == 'reference')
                model.configure(controls)
                mix, consistency = float(smoothed[3]), float(smoothed[4])
                if previous_mix != mix:
                    model.update_controller.reset_cache()
                previous_progress = progress
                encode_seconds = 0.0
                prompt_changed = settings.prompt != applied.prompt or settings.prompt_b != applied.prompt_b
                seed_changed = settings.seed != applied.seed
                style_changed = settings.reference is not applied.reference
                reset_feedback = (force_cut or settings.reset != applied.reset
                                  or (not consistency and previous_consistency)
                                  or ((prompt_changed or seed_changed or style_changed)
                                      and (settings.transition == 'cut' or settings.duration <= 0)))
                if reset_feedback:
                    feedback.clear()
                    model.previous_frame = None
                    model.update_controller.reset_cache()
                elif settings.reference_role != applied.reference_role:
                    feedback.clear()
                if prompt_changed:
                    if settings.prompt != applied.prompt:
                        model.update_prompt_embeds(settings.prompt)
                        embeds_a = model.prompt_embeds
                    if settings.prompt_b and settings.prompt_b != applied.prompt_b:
                        model.update_prompt_embeds(settings.prompt_b)
                        embeds_b = model.prompt_embeds
                    encode_seconds = time.monotonic() - start
                target = embeds_a if not settings.prompt_b else embeds_a*(1-mix)+embeds_b*mix
                if seed_changed:
                    target_noise = model.noise_for_seed(settings.seed)
                if style_changed and settings.reference is not None:
                    target_style = settings.reference.astype(np.float32)
                    if current_style is None:
                        current_style = target_style
                if prompt_changed or seed_changed or style_changed:
                    blend_from = current
                    noise_from = noise
                    style_from = current_style
                    blend_start = time.monotonic()
                    previous_progress = 0.0
                elif blend_from is not None and settings.duration != applied.duration:
                    blend_from = current
                    noise_from = noise
                    style_from = current_style
                    blend_start = time.monotonic()
                    previous_progress = 0.0
                if blend_from is not None:
                    if force_cut or settings.transition == 'cut' or settings.duration <= 0 or not has_generated:
                        progress = previous_progress = 1.0
                        model.previous_frame = None
                    else:
                        progress = min(1.0, (time.monotonic() - blend_start) / settings.duration)
                    current = target if progress == 1.0 else blend_from * (1-progress) + target * progress
                    noise = target_noise if progress == 1.0 else model.blend_noise(noise_from, target_noise, progress)
                    if target_style is not None:
                        current_style = target_style if progress == 1.0 else style_from*(1-progress) + target_style*progress
                        model.reference_image = Image.fromarray(np.rint(current_style).astype(np.uint8))
                    model.prompt_embeds = current
                    model.update_controller.reset_cache()
                    if progress == 1.0:
                        blend_from = None
                        noise_from = None
                        style_from = None
                else:
                    current = target
                    model.prompt_embeds = current
                if force_cut:
                    current = target
                    noise = target_noise
                    model.prompt_embeds = current
                    if target_style is not None:
                        current_style = target_style
                        model.reference_image = Image.fromarray(np.rint(current_style).astype(np.uint8))
                    blend_from = None
                    progress = previous_progress = 1.0
                if model.previous_frame is None:
                    previous_progress = progress
                model.process_state['seed'] = settings.seed
                model.noise = noise
                model.interpolation_exp = 2 if settings.frames == 4 else 3
                applied = settings
                source = resize_image(image, settings.controls.width, settings.controls.height)
                conditioned, accepted = feedback.prepare(source, consistency, settings.align_motion)
                if consistency:
                    # Feedback must reach inference even when the raw source is unchanged.
                    model.update_controller.reset_cache()
                frame = model.process_frame_with_pipeline(conditioned)
                has_generated = True
                if consistency:
                    feedback.remember(source, frame)
                if settings.frames == 1:
                    frames = frame[None, ..., ::-1]
                    model.previous_frame = None
                else:
                    frames = model.interpolate_frames(model.convert_np_to_torch(frame))
                elapsed = time.monotonic() - start
                with self.cv:
                    if self.closed:
                        return
                    if self.paused:
                        continue
                    self.pending = (frames, elapsed, previous_progress,
                                    {'prompt': settings.prompt, 'seed': settings.seed, 'blend_progress': progress,
                                     'batch_seconds': elapsed, 'encode_seconds': encode_seconds,
                                     'rife_frames': settings.frames, 'transition': settings.transition,
                                     'consistency': consistency, 'feedback_coverage': accepted,
                                     'align_motion': settings.align_motion,
                                     'feedback_reset': settings.reset,
                                     'width': settings.controls.width, 'height': settings.controls.height,
                                     'steps': controls.steps, 'guidance': controls.guidance,
                                     'source_strength': controls.source, 'style_strength': controls.style,
                                     'prompt_b': settings.prompt_b, 'prompt_mix': mix,
                                     'reference_enabled': active_reference, 'reference_role': settings.reference_role,
                                     'control_smoothing': settings.smooth, 'cut': settings.cut,
                                     'spatial_cache': settings.controls.steps <= 4,
                                     'generation_fps': 1/elapsed})
                    self.cv.notify_all()
        except Exception as error:
            with self.cv:
                self.error = f'{type(error).__name__}: {error}'
                self.cv.notify_all()
        finally:
            model = current = target = blend_from = None
            embeds_a = embeds_b = None
            noise = target_noise = noise_from = None
            current_style = target_style = style_from = None
            Inference.release()

    def play(self):
        previous = None
        previous_cut = None
        while True:
            with self.cv:
                self.cv.wait_for(lambda: self.closed or self.error or (not self.paused and self.pending is not None))
                if self.closed or self.error:
                    return
                frames, elapsed, previous_progress, metadata = self.pending
                self.pending = None
                target_fps = self.settings.output_fps
            start = time.monotonic()
            # A batch remains private while it plays. The next pending batch can be replaced.
            count = max(1, math.ceil(elapsed * target_fps)) if target_fps else len(frames)
            interval = 1/target_fps if target_fps else elapsed / count
            origin = frames[0] if previous is None or previous_cut != metadata['cut'] else previous
            previous_cut = metadata['cut']
            for i in range(count):
                with self.cv:
                    while not self.closed and not self.error and not self.paused:
                        wait = start + (i+1) * interval - time.monotonic()
                        if wait <= 0:
                            break
                        self.cv.wait(wait)
                    if self.closed or self.error:
                        return
                    if self.paused:
                        previous = None
                        break
                    if i < count-1 and time.monotonic() > start + (i+2)*interval:
                        continue
                    position = (i+1)*len(frames)/count
                    right = min(len(frames)-1, math.ceil(position)-1)
                    left = origin if right == 0 else frames[right-1]
                    alpha = position-right
                    frame = frames[right] if not target_fps else left.astype(np.float32)*(1-alpha)+frames[right]*alpha
                    rgb = np.ascontiguousarray(frame[..., ::-1], dtype=np.float32) / 255.0
                    progress = previous_progress + (metadata['blend_progress']-previous_progress) * (i+1)/count
                    info = {**metadata, 'blend_progress': progress, 'target_fps': target_fps,
                            'playback': 'rife + blend' if target_fps and len(frames) > 1 else
                                        'blend' if target_fps else 'rife' if len(frames) > 1 else 'direct'}
                    phase = 'Ready' if progress == 1.0 else f'Blending {progress:.0%}'
                    playback = 'RIFE off' if len(frames) == 1 else f'RIFE x{len(frames)} | {len(frames)/elapsed:.1f} FPS playback target'
                    if target_fps:
                        playback = f"{info['playback']} | {target_fps:g} FPS target"
                    self.status = (f'{1/elapsed:.2f} FPS generated (batch rate)\n'
                                   f"{metadata['width']} x {metadata['height']} | {metadata['steps']} steps\n"
                                   f'{phase} | {playback}')
                    if metadata['consistency']:
                        self.status += (f"\nFeedback {metadata['consistency']:.0%} | "
                                        f"accepted {metadata['feedback_coverage']:.0%}")
                    self.output = (rgb, {'channels': {'dim2': ['r', 'g', 'b']}, 'fluxrt': info})
                    previous = frames[-1]

    def stop(self):
        self.request_stop()
        self.present.join(timeout=1.0)
        self.infer.join(timeout=1.0)
        # goofi owns the subprocess. Its exit also releases a model still in a CUDA call.


class FluxRT(goofi.Node):
    """Edit a video or graphics stream. The 16 GB preset trades frame rate for memory."""
    TAGS = ['image', 'ml', 'transform']
    INPUTS = {'source': goofi.InputSlot(goofi.DataType.ARRAY, trigger=False, required=False),
              'reference': goofi.InputSlot(goofi.DataType.ARRAY, trigger=False, required=False)}
    OUTPUTS = {'image': goofi.DataType.ARRAY, 'status': goofi.DataType.STRING}
    PARAMS = {
        'runtime': {'state': goofi.StringParam('stop', options=['start', 'pause', 'stop'],
                    doc='Start loads and generates. Pause retains the model. Stop finishes active work and releases the model.'),
                    'memory_budget': goofi.FloatParam(18.5, 18.5, 32.0,
                    doc='Estimated total worker commit budget in GiB on Windows. Startup subtracts memory already held by this worker, with at least 2 GiB free required. Wait and retry if headroom is insufficient. Increase for more margin; set before Start.')},
        'model': {
            'root': goofi.StringParam('', doc='Absolute path to the prepared FluxRT checkout. Restart after changing it.'),
            'prompt': goofi.StringParam('Turn this into watercolor art.', doc='Editing instruction. Apply changes at an inference boundary.'),
            'prompt_b': goofi.StringParam('', doc='Second editing instruction. Empty uses prompt A at both ends. Encoded only when its text changes.'),
            'prompt_mix': goofi.FloatParam(0.0, 0.0, 1.0, doc='Blend prompt embeddings: 0 is prompt A, 1 is prompt B. Uses control smoothing; does not add a model pass.'),
            'seed': goofi.IntParam(52, 0, 2147483647, doc='Keep fixed for continuity. Changes blend noise over transition/duration, with RIFE between generated frames.'),
            'steps': goofi.IntParam(2, 1, 8, doc='Denoising steps. More steps cost time; above four, spatial caching is disabled to limit VRAM.'),
            'guidance': goofi.FloatParam(1.0, 1.0, 3.0, doc='Prompt guidance (CFG). Above one adds a second model pass per step and can exaggerate colors and detail.'),
        },
        'source': {
            'width': goofi.IntParam(320, 256, 512, options=[256, 320, 384, 448, 512], doc='Output width. Set before generation; restart the node to change size.'),
            'height': goofi.IntParam(320, 256, 512, options=[256, 320, 384, 448, 512], doc='Output height. Set before generation; restart the node to change size.'),
            'influence': goofi.FloatParam(1.0, 0.0, 2.0, doc='Source-image attention weight. Zero blocks source tokens, one is normal, two increases their influence. Not a denoising-strength percentage.'),
            'freeze': goofi.BoolParam(False, doc='Hold the current guide frame while controls remain live. Unfreeze accepts new input. Start while frozen captures the first frame.'),
        },
        'reference': {'enabled': goofi.BoolParam(False, doc='Enable the captured reference live. Disable fades its weight to zero, then removes its image tokens.'),
                  'main_image': goofi.StringParam('source', options=['source', 'reference'], doc='Choose which image is the main one to transform. The other supplies additional conditioning. Update prompt instructions to match these roles. Changes apply between generations.'),
                  'capture': goofi.PulseParam(doc='Capture the current reference input and blend the reference over transition/duration. Cut mode or zero duration applies it immediately.'),
                  'influence': goofi.FloatParam(1.0, 0.0, 8.0, doc='Reference attention weight. One is normal. Values above two are experimental and can copy reference objects; this is not pure style transfer.')},
        'smoothing': {'time': goofi.FloatParam(0.25, 0.0, 5.0, doc='Seconds to approach 95% of a new CFG, image weight, reference weight, prompt mix or feedback target. Zero is immediate. New targets do not queue.'),
                      'output_fps': goofi.FloatParam(0.0, 0.0, 60.0,
                      doc='Target playback FPS. Zero delivers only the selected RIFE frames. Above zero blends between RIFE frames on a paced clock without extra model passes. Applies at the next batch; can soften detail or ghost motion. Does not increase generation FPS. Keep common/max_frequency above this target.')},
        'transition': {
            'mode': goofi.StringParam('prompt blend', options=['cut', 'prompt blend'], doc='Blend prompts, seed noise, and reference images with RIFE, or cut to the new settings.'),
            'duration': goofi.FloatParam(2.5, 0.0, 60.0, doc='Seconds for prompt, seed, and reference blends. Encoding, inference and playback add delay. Zero cuts. Changes restart the remaining blend from its current state.'),
            'frames': goofi.IntParam(4, 1, 8, options=[1, 4, 8], doc='1 disables RIFE; 4 or 8 adds interpolated output frames. More frames do not increase model generation speed.'),
            'cut': goofi.PulseParam(doc='Immediately apply all current targets and clear feedback and RIFE history at the next generation. Skip the current transition.'),
        },
        'consistency': {
            'strength': goofi.FloatParam(0.0, 0.0, 0.8, doc='Previous-output contribution to the source. Zero disables feedback. Start near 0.2; high values can cause drift or trails and reduce generation speed.'),
            'align_motion': goofi.BoolParam(True, doc='Warp feedback with source motion. Reject uncertain motion, changed regions, and exposed borders.'),
            'reset': goofi.PulseParam(doc='Clear feedback, model cache, and RIFE history at the next inference boundary.'),
        },
        'common': {'autotrigger': goofi.BoolParam(True),
                   'max_frequency': goofi.FloatParam(60.0, 1.0, 120.0)},
    }

    def setup(self):
        import faulthandler
        faulthandler.enable()
        self.runtime = None
        self.root = None
        self.reference = None
        self.capture_style = False
        self.feedback_reset = 0
        self.cut_generation = 0
        self.output_times = deque()

    def pulse_reference_capture(self):
        self.capture_style = True

    def pulse_consistency_reset(self):
        self.feedback_reset += 1

    def pulse_transition_cut(self):
        self.cut_generation += 1

    def process(self, source=None, reference=None):
        image, style = source, reference
        state = self.params.runtime.state
        if state not in ('start', 'pause', 'stop'):
            raise ValueError('Set runtime/state to start, pause, or stop')
        if self.runtime is not None:
            if state == 'stop':
                self.runtime.request_stop()
            if self.runtime.closed:
                if self.runtime.infer.is_alive() or self.runtime.present.is_alive():
                    return {'status': 'Stopping — waiting for active model work'}
                self.runtime = None
                self.reference = self.root = None
                self.output_times.clear()
            elif self.runtime.error:
                raise ValueError(self.runtime.error)
        if state != 'start':
            if self.runtime is not None:
                self.runtime.pause(True)
            return {'status': 'Paused — model retained' if self.runtime is not None else 'Stopped — select start to load'}
        root = self.params.model.root
        if self.runtime is not None and root != self.root:
            raise ValueError('Restart FluxRT after changing model/root')
        enabled = self.params.reference.enabled
        if self.runtime is not None:
            size = self.runtime.config['resolution']
            if (self.params.source.width, self.params.source.height) != (size['width'], size['height']):
                raise ValueError('Restart FluxRT after changing source/width or source/height')
        if enabled and style is not None and (self.reference is None or self.capture_style):
            self.reference = resize_image(self.prepare_image(style.data), self.params.source.width, self.params.source.height)
            self.capture_style = False
        if enabled and self.reference is None:
            return {'status': 'Waiting for reference image'}
        settings = Settings(self.params.model.prompt, self.params.model.seed, self.reference,
                            self.params.transition.mode, self.params.transition.duration, self.params.transition.frames,
                            Controls(self.params.model.steps, self.params.model.guidance, self.params.source.influence,
                                     self.params.reference.influence, self.params.source.width, self.params.source.height),
                            self.params.consistency.strength, self.params.consistency.align_motion, self.feedback_reset,
                            self.params.model.prompt_b, self.params.model.prompt_mix, self.params.smoothing.time,
                            enabled, self.params.reference.main_image, self.cut_generation, self.params.smoothing.output_fps)
        if settings.frames not in (1, 4, 8):
            raise ValueError('Set transition/frames to 1, 4 or 8')
        if settings.reference_role not in ('source', 'reference'):
            raise ValueError('Set reference/main_image to source or reference')
        if settings.transition not in ('cut', 'prompt blend'):
            raise ValueError('Set transition/mode to cut or prompt blend')
        if any(size < 256 or size > 512 or size % 32 for size in (settings.controls.width, settings.controls.height)):
            raise ValueError('Set source/width and source/height to multiples of 32 between 256 and 512')
        incoming = None
        if image is not None and (self.runtime is None or not self.params.source.freeze):
            incoming = self.prepare_image(image.data)
        if self.runtime is None:
            if incoming is None:
                return {'status': 'Waiting for image'}
            path = Path(root)
            if not root or not path.is_absolute():
                raise ValueError('Set model/root to the absolute path of the prepared FluxRT checkout')
            config = json.loads((path / 'configs/benchmark_config.json').read_text())
            config.update(models_path=str(path / 'FLUX.2-klein-4B'),
                          memory_budget=self.params.runtime.memory_budget,
                          int8_models_path=str(path / 'FLUX.2-klein-4B-int8'),
                          rife_path=str(path / 'RIFE-safetensors/flownet.safetensors'),
                          resolution={'height': settings.controls.height, 'width': settings.controls.width},
                          enable_int8_quantization=True, compile_models=False,
                          enable_spatial_cache=settings.controls.steps <= 4, interpolation_exp=2 if settings.frames == 4 else 3,
                          enable_tiny_vae=False, enable_flow_upscaler=False,
                          use_reference_image=enabled,
                          reference_image_resolution={'height': 256, 'width': 256},
                          target_fps=None, logging=False,
                          default_prompt=settings.prompt, default_seed=settings.seed, default_steps=settings.controls.steps)
            self.runtime = Runtime(config, incoming, settings)
            self.root = root
        else:
            self.runtime.submit(incoming, settings)
            self.runtime.pause(False)
        if image is not None:
            self.clear_input('source')
        output, status, error = self.runtime.take()
        if error:
            raise ValueError(error)
        now = time.monotonic()
        if output is not None:
            self.output_times.append(now)
        while self.output_times and self.output_times[0] < now-2:
            self.output_times.popleft()
        fps = len(self.output_times)/2.0
        result = {'status': f'{fps:.1f} FPS output (delivered)\n{status}'}
        if output is not None:
            data, meta = output
            result['image'] = (data, {**meta, 'fluxrt': {**meta['fluxrt'], 'output_fps': fps}})
        return result

    @staticmethod
    def prepare_image(data, size=None):
        data = np.asarray(data)
        if data.ndim != 3 or data.shape[2] != 3 or not data.size or not np.isfinite(data).all():
            raise ValueError('FluxRT needs a finite RGB image with shape [height, width, 3]')
        if data.min() < 0 or data.max() > 1:
            raise ValueError('FluxRT needs RGB values in 0..1')
        data = np.ascontiguousarray(data*255.0, dtype=np.uint8)
        return resize_image(data, size, size) if size is not None else data

    def stop(self):
        if self.runtime is not None:
            self.runtime.stop()


def resize_image(image, width, height):
    """Center-crop the retained source to the output aspect ratio."""
    h, w = image.shape[:2]
    crop_w, crop_h = min(w, max(1, round(h*width/height))), min(h, max(1, round(w*height/width)))
    crop = image[(h-crop_h)//2:(h-crop_h)//2+crop_h, (w-crop_w)//2:(w-crop_w)//2+crop_w]
    return np.ascontiguousarray(cv2.resize(crop, (width, height)))
