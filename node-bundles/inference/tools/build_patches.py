"""Build the inference example patches through goofi's own ops, run each, read what its nodes say,
and save it. Needs a running goofi that has this bundle (and, for the material patch, latent-texture's
MaterialSpace) on its node roots, named by GOOFI_SESSION.

    GOOFI_SESSION=... python node-bundles/inference/tools/build_patches.py OUT_DIR [patch ...]

The EEG is goofi's sample `eeg-rest-srm.edf`, which EegPlayback fetches on first use. The material
patch reads a photograph from PHOTO, set in this file.
"""

import json
import pathlib
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[3]
GOOFI = ROOT / "target" / "debug" / ("goofi.exe" if sys.platform == "win32" else "goofi")
PHOTO = "photo.jpg"  # the material patch's photograph: set this to any picture on disk


def op(*args, check=True):
    r = subprocess.run([str(GOOFI), *map(str, args)], capture_output=True, text=True)
    text = (r.stdout + r.stderr).strip()
    if check and r.returncode != 0:
        raise RuntimeError(f"goofi {' '.join(map(str, args))}: {text}")
    return text


def add(kind, name, x, y, **params):
    op("node", "add", kind, "--name", name, "--pos", f"{x},{y}")
    for key, v in params.items():
        group, param = key.split("__")
        if isinstance(v, str) and v.startswith("="):
            op("node", "param", "edit", name, f"{group}/{param}", "--expression", v[1:])
        else:
            op("node", "param", "edit", name, f"{group}/{param}", "--value", json.dumps(v) if not isinstance(v, str) else v)


def link(a, b):
    op("link", "add", a, b)


def eeg_alpha(prefix, x, y, rate=4.0, channels=""):
    """EEG playback to alpha power per channel, four times a second."""
    add("signal:EegPlayback", f"{prefix}eeg", x, y, common__max_frequency=rate * 4)  # goofi's sample recording
    add("signal:EegPreprocess", f"{prefix}clean", x + 110, y + 120, eeg__low=1.0, eeg__high=40.0, eeg__notch="50")
    add("signal:Buffer", f"{prefix}window", x + 220, y, buffer__size=1.0)
    add("signal:Psd", f"{prefix}spectrum", x + 440, y, common__max_frequency=rate)
    add("signal:EegPowerBands", f"{prefix}bands", x + 660, y, bands__relative=True)
    add("signal:Select", f"{prefix}alpha", x + 880, y, select__keep="alpha", select__axis=-1)
    link(f"{prefix}eeg/out", f"{prefix}clean/input")
    link(f"{prefix}clean/out", f"{prefix}window/input")
    link(f"{prefix}window/out", f"{prefix}spectrum/input")
    link(f"{prefix}spectrum/out", f"{prefix}bands/psd")
    link(f"{prefix}bands/power", f"{prefix}alpha/input")
    if channels:
        add("signal:Select", f"{prefix}chans", x + 1100, y, select__keep=channels, select__axis=0)
        link(f"{prefix}alpha/out", f"{prefix}chans/input")
        return f"{prefix}chans/out"
    return f"{prefix}alpha/out"


# ---- the patches --------------------------------------------------------------------------------

def surprise_map():
    """Where the brain surprises its own model. Resting EEG's alpha per channel into an HGF; its
    surprise and volatility drawn as topomaps, the mean surprise as a line."""
    alpha = eeg_alpha("", 0, 0)
    add("signal:Belief", "belief", 1100, 0, belief__method="hgf", input__rate=4.0, input__log=True)
    link(alpha, "belief/signal")
    add("graphics:SignalIn", "surpriseMap", 1350, -150, signal__mode="topomap", signal__colormap="magma")
    add("graphics:SignalIn", "volatilityMap", 1350, 150, signal__mode="topomap", signal__colormap="viridis")
    link("belief/surprise", "surpriseMap/input")
    link("belief/volatility", "volatilityMap/input")
    add("signal:GraphicsIn", "surpriseImage", 1600, -150)
    link("surpriseMap/out", "surpriseImage/input")
    return ["belief/status", "belief/surprise", "belief/volatility", "surpriseImage/out"], 30


def agent_loop():
    """A machine learning to raise a simulated person's alpha. A Jansen-Rit neural mass stands for the
    person. The agent's action is added to the mass's input, the mass's alpha power is the agent's
    observation through an HGF, and the agent learns which of its three actions raises alpha."""
    add("signal:NeuralMass", "person", 0, 0, mass__model="jansenrit", output__mode="block", sim__seed=3)
    add("signal:Buffer", "window", 220, 0, buffer__size=2.0)
    add("signal:Psd", "spectrum", 440, 0, common__max_frequency=4.0)
    add("signal:EegPowerBands", "bands", 660, 0, bands__relative=True)
    add("signal:Select", "alpha", 880, 0, select__keep="alpha", select__axis=-1)
    add("signal:Belief", "belief", 1100, 0, belief__method="hgf", input__rate=4.0, input__log=True)
    add("signal:ActiveAgent", "agent", 1320, 0, prefer__target=0.9, plan__period=1.0, plan__depth=2, plan__seed=1)
    link("person/out", "window/input")
    link("window/out", "spectrum/input")
    link("spectrum/out", "bands/psd")
    link("bands/power", "alpha/input")
    link("alpha/out", "belief/signal")
    link("belief/mean", "agent/observation")
    link("agent/level", "person/input")
    add("graphics:SignalIn", "beliefLine", 1550, -150, signal__mode="line")
    link("belief/mean", "beliefLine/input")
    add("graphics:SignalIn", "choiceBars", 1550, 150, signal__mode="line")
    link("agent/choice", "choiceBars/input")
    return ["belief/status", "agent/status", "agent/choice", "agent/belief", "person/out"], 90


def coupled_blankets():
    """Person and machine, each inside its own blanket. The person is resting EEG (posterior alpha
    through an HGF). The machine is a ring of Kuramoto oscillators whose coupling follows the person's
    alpha belief, so the person acts on the machine. A second HGF is the machine's model of itself
    (its order parameter). Blanket reads what passes between the two beliefs, and which way."""
    alpha = eeg_alpha("", 0, 0, channels="<eeg-parietal>,<eeg-occipital>")
    add("signal:Belief", "person", 1320, 0, belief__method="hgf", input__rate=4.0, input__log=True)
    link(alpha, "person/signal")
    add("signal:Reduce", "personMean", 1540, 0, reduce__axes="")
    link("person/mean", "personMean/input")
    add("signal:Kuramoto", "machine", 1540, 250, kuramoto__size=16, kuramoto__spread=0.3,
        kuramoto__coupling="=max(0.0, 1.5 + 2.0 * float(nd('personMean').out.out.mean()))", sim__seed=2)
    add("signal:Belief", "machineModel", 1760, 250, belief__method="hgf", input__rate=4.0, common__max_frequency=4.0)
    link("machine/order", "machineModel/signal")
    add("signal:Blanket", "blanket", 1980, 100, blanket__window=400, blanket__every=4, blanket__lag=2)
    link("personMean/out", "blanket/internal")
    link("machineModel/mean", "blanket/external")
    add("graphics:SignalIn", "influence", 2200, 100, signal__mode="line")
    link("blanket/influence", "influence/input")
    return ["person/status", "machine/order", "machineModel/status", "blanket/status", "blanket/influence"], 150


def material_blanket():
    """The blanket as material. A photograph's light is masked by AdaptiveMask, and the person's belief
    shapes the boundary: surprise frays its edge (noise) and precision softens it. Two band beliefs
    walk MaterialSpace's map, and the material it hands on is laid into the mask on the GPU."""
    alpha = eeg_alpha("", 0, 0, channels="<eeg-occipital>")
    add("signal:Belief", "belief", 1320, 0, belief__method="hgf", input__rate=4.0, input__log=True)
    link(alpha, "belief/signal")
    add("signal:Reduce", "surprise", 1540, -100, reduce__axes="")
    add("signal:Reduce", "meanBelief", 1540, 100, reduce__axes="")
    link("belief/surprise", "surprise/input")
    link("belief/mean", "meanBelief/input")
    add("signal:ImageFile", "photo", 0, 400, file__path=PHOTO, image__max_size=768)
    add("signal:AdaptiveMask", "boundary", 1760, 400, mask__by="picture", mask__picture="light", mask__coverage=0.45,
        edges__noise="=min(1.0, 0.15 * float(nd('surprise').out.out.mean()))", frame__transition=1.0)
    link("photo/image", "boundary/image")
    add("signal:MaterialSpace", "archive", 1760, 700, walk__mode="map", walk__pace=0.0,
        walk__axis1="=0.5 + 0.2 * float(nd('meanBelief').out.out.mean())")
    add("graphics:SignalIn", "photoTex", 2000, 400, signal__mode="texture")
    add("graphics:SignalIn", "materialTex", 2000, 700, signal__mode="texture")
    add("graphics:SignalIn", "maskTex", 2000, 550, signal__mode="texture", signal__colormap="gray", signal__autoscale=False,
        signal__min=0.0, signal__max=1.0)
    add("graphics:Composite", "laidIn", 2240, 550, composite__mode="luminosity")
    link("photo/image", "photoTex/input")
    link("archive/picture", "materialTex/input")
    link("boundary/mask", "maskTex/input")
    link("materialTex/out", "laidIn/a")
    link("photoTex/out", "laidIn/b")
    link("maskTex/out", "laidIn/mask")
    add("signal:GraphicsIn", "laidImage", 2480, 550)
    link("laidIn/out", "laidImage/input")
    return ["belief/status", "boundary/status", "archive/status", "laidImage/out"], 60


PATCHES = {"surprise-map": surprise_map, "agent-loop": agent_loop, "coupled-blankets": coupled_blankets,
           "material-blanket": material_blanket}


def snapshot(output):
    """An output's latest frame: an array read raw (NPY), a text read as JSON. Asking opens the slot's
    feed, so it is asked, then asked again."""
    import io

    import numpy as np

    last = None
    for _ in range(3):
        r = subprocess.run([str(GOOFI), "node", "snapshot", output, "--raw"], capture_output=True)
        if r.stdout[1:6] == b"NUMPY":
            last = np.load(io.BytesIO(r.stdout))
        else:
            try:
                d = json.loads(r.stdout.decode("utf-8", "replace"))
                if "value" in d:
                    last = d["value"]
            except json.JSONDecodeError:
                pass
        time.sleep(2.0)
    if last is None:
        return f"nothing cached ({op('node', 'state', output.split('/')[0], '--no-params', check=False).splitlines()[0]})"
    return last


def summary(v):
    if isinstance(v, str):
        return v
    import numpy as np

    a = np.asarray(v, dtype=float)
    if a.size > 8:
        return f"array {list(a.shape)}, min {a.min():.3f}, mean {a.mean():.3f}, max {a.max():.3f}"
    return f"array {list(a.shape)}, {np.round(a.reshape(-1), 3).tolist()}"


def build(out_dir, names):
    out_dir.mkdir(parents=True, exist_ok=True)
    for name in names:
        op("session", "new")
        watch, seconds = PATCHES[name]()
        print(f"== {name}: running {seconds} s", flush=True)
        for out in watch:  # open every feed now, so the reads after the run find frames
            op("node", "snapshot", out, check=False)
        time.sleep(seconds)
        for out in watch:
            print(f"  {out}: {summary(snapshot(out))}", flush=True)
        path = out_dir / f"{list(PATCHES).index(name) + 1:02d}-{name}.gfi"
        print(" ", op("session", "save", path.as_posix(), "--overwrite")[:200], flush=True)


if __name__ == "__main__":
    build(pathlib.Path(sys.argv[1]), sys.argv[2:] or list(PATCHES))
