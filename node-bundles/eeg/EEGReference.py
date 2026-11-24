"""EEGReference — what an EEG channel's voltage is measured AGAINST.

Every EEG number is a difference between two electrodes, so the reference is part of the
measurement and not a detail of it. Re-referencing subtracts a different thing from every channel;
`[C, T]` in, `[C, T]` out, with the channel names kept.

Which one to pick is a real choice: the average is the default for dense caps, the median is the
same idea with one bad electrode unable to spread itself over the rest, and `channel` is what you
want when a recording has a physical reference you would rather use. The Laplacian places each
channel by its 10-20 name, the way the brain viewers do, so it needs named channels and no montage.
"""

import numpy as np
import goofi

MODES = ["average", "median", "channel", "laplacian", "bipolar", "none"]

# The curated 10-20 layout the viewers draw: azimuthal-equidistant on the unit square, Cz centred.
LAYOUT = {
    "Fpz": (0.5, 0.05), "Fz": (0.5, 0.27), "Cz": (0.5, 0.5), "Pz": (0.5, 0.73), "Oz": (0.5, 0.95),
    "Fp1": (0.35, 0.07), "Fp2": (0.65, 0.07), "AF3": (0.36, 0.18), "AF4": (0.64, 0.18),
    "AF7": (0.27, 0.13), "AF8": (0.73, 0.13), "F3": (0.33, 0.27), "F4": (0.67, 0.27),
    "F5": (0.27, 0.27), "F6": (0.73, 0.27), "F7": (0.17, 0.25), "F8": (0.83, 0.25),
    "F1": (0.42, 0.27), "F2": (0.58, 0.27), "C3": (0.27, 0.5), "C4": (0.73, 0.5),
    "C5": (0.16, 0.5), "C6": (0.84, 0.5), "C1": (0.39, 0.5), "C2": (0.61, 0.5),
    "FC3": (0.31, 0.38), "FC4": (0.69, 0.38), "FC5": (0.22, 0.38), "FC6": (0.78, 0.38),
    "FC1": (0.41, 0.38), "FC2": (0.59, 0.38), "T7": (0.07, 0.5), "T8": (0.93, 0.5),
    "T3": (0.07, 0.5), "T4": (0.93, 0.5), "CP3": (0.31, 0.62), "CP4": (0.69, 0.62),
    "CP5": (0.22, 0.62), "CP6": (0.78, 0.62), "CP1": (0.41, 0.62), "CP2": (0.59, 0.62),
    "P3": (0.33, 0.73), "P4": (0.67, 0.73), "P5": (0.27, 0.73), "P6": (0.73, 0.73),
    "P7": (0.17, 0.75), "P8": (0.83, 0.75), "P1": (0.42, 0.73), "P2": (0.58, 0.73),
    "O1": (0.4, 0.92), "O2": (0.6, 0.92), "PO3": (0.36, 0.83), "PO4": (0.64, 0.83),
    "PO7": (0.27, 0.87), "PO8": (0.73, 0.87), "POz": (0.5, 0.83), "T5": (0.17, 0.75),
    "T6": (0.83, 0.75), "A1": (0.0, 0.5), "A2": (1.0, 0.5),
}
HEAD_RADIUS = 0.45
BY_NAME = {name.lower(): place for name, place in LAYOUT.items()}


def lift(place):
    """An electrode's point on the unit sphere (x right, y anterior, z up), from its layout place."""
    dx, dy = place[0] - 0.5, 0.5 - place[1]
    polar = min(1.0, np.hypot(dx, dy) / HEAD_RADIUS) * (np.pi / 2)
    azimuth = np.arctan2(dy, dx)
    return (np.sin(polar) * np.cos(azimuth), np.sin(polar) * np.sin(azimuth), np.cos(polar))


def electrode_at(name):
    """The layout's place for a channel name, whatever its case; `None` for a name it lacks."""
    return BY_NAME.get(name.lower())


class EEGReference(goofi.Node):
    """Re-reference an EEG frame.

    Average, median, one electrode, a neighbour-difference Laplacian, or a bipolar chain.
    """

    TAGS = ["transform", "eeg"]
    INPUTS = {"data": goofi.InputSlot(goofi.DataType.ARRAY, required=True)}
    OUTPUTS = {"data": goofi.DataType.ARRAY}
    PARAMS = {
        "reference": {
            "mode": goofi.StringParam("average", options=MODES, doc="What to subtract from every channel."),
            "channel": goofi.StringParam(
                "", show=("mode", ["channel"]), doc="The channel to reference against: its name, or a number counting from 0."
            ),
            "neighbours": goofi.NumParam(
                4, 1, 16, int=True, show=("mode", ["laplacian"]),
                doc="How many nearest electrodes to average. A channel without a 10-20 name is left as it is.",
            ),
            "exclude": goofi.StringParam(
                "", show=("mode", ["average", "median"]), doc="Channels to leave out of the reference, comma separated."
            ),
        }
    }

    def process(self, data):
        p = self.params.reference
        x = np.squeeze(np.asarray(data.data, dtype=np.float64))
        if x.ndim != 2:
            raise ValueError(f"needs channels by time, got {list(np.shape(data.data))}")
        channels = x.shape[0]
        names = list(data.meta.get("channels", {}).get("dim0") or [])

        mode = p.mode
        if mode == "none":
            out = x
        elif mode in ("average", "median"):
            dropped = {n.strip() for n in p.exclude.split(",") if n.strip()}
            kept = np.array([i for i in range(channels) if (names[i] if i < len(names) else str(i)) not in dropped])
            if kept.size == 0:
                raise ValueError("`exclude` left no channels to build a reference from")
            centre = x[kept].mean(axis=0, keepdims=True) if mode == "average" else np.median(x[kept], axis=0, keepdims=True)
            out = x - centre
        elif mode == "channel":
            pick = p.channel.strip()
            if not pick:
                raise ValueError("`channel` mode needs a channel to reference against")
            if pick in names:
                index = names.index(pick)
            elif pick.isdigit() and int(pick) < channels:
                index = int(pick)
            else:
                raise ValueError(f"no channel `{pick}`; this frame has {names or channels}")
            out = x - x[index : index + 1]
        elif mode == "bipolar":
            # Each channel against the next, so a chain of C electrodes gives C-1 derivations.
            out = x[:-1] - x[1:]
            names = [f"{a}-{b}" for a, b in zip(names, names[1:])] if names else []
        else:
            placed = [(i, electrode_at(str(n))) for i, n in enumerate(names[:channels])]
            known = [i for i, place in placed if place is not None]
            if len(known) < 2:
                raise ValueError(f"`laplacian` needs channels named for 10-20 electrodes; this frame has {names or channels}")
            pos = np.array([lift(place) for _, place in placed if place is not None])
            distance = np.linalg.norm(pos[:, None, :] - pos[None, :, :], axis=-1)
            np.fill_diagonal(distance, np.inf)
            near = np.argsort(distance, axis=1)[:, : min(int(p.neighbours), len(known) - 1)]
            out = x.copy()
            out[known] = x[known] - x[known][near].mean(axis=1)

        meta = dict(data.meta)
        axes = dict(meta.get("channels", {}))
        if names:
            axes["dim0"] = names
        elif "dim0" in axes:
            del axes["dim0"]
        meta["channels"] = axes
        return out.astype(np.float32), meta
