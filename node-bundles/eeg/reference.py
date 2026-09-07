"""Reference — what an EEG channel's voltage is measured AGAINST.

Every EEG number is a difference between two electrodes, so the reference is part of the
measurement and not a detail of it. Re-referencing subtracts a different thing from every channel;
`[C, T]` in, `[C, T]` out, with the channel names kept.

Which one to pick is a real choice: the average is the default for dense caps, the median is the
same idea with one bad electrode unable to spread itself over the rest, and `channel` is what you
want when a recording has a physical reference you would rather use.
"""

import numpy as np
import goofi

MODES = ["average", "median", "channel", "laplacian", "bipolar", "none"]


class Reference(goofi.Node):
    """Re-reference an EEG frame.

    Average, median, one electrode, a neighbour-difference Laplacian, or a bipolar chain.
    """

    TAGS = ["transform", "eeg"]
    INPUTS = {
        "data": goofi.InputSlot(goofi.DataType.ARRAY, required=True),
        "montage": goofi.InputSlot(goofi.DataType.ARRAY, required=False),
    }
    OUTPUTS = {"data": goofi.DataType.ARRAY}
    PARAMS = {
        "reference": {
            "mode": goofi.StringParam("average", options=MODES, doc="What to subtract from every channel."),
            "channel": goofi.StringParam("", doc="For `channel`: the name, or a number counting from 0."),
            "neighbours": goofi.IntParam(4, 1, 16, doc="For `laplacian`: how many nearest electrodes to average."),
            "exclude": goofi.StringParam("", doc="Channels to leave out of the reference, comma separated."),
        }
    }

    def process(self, data, montage=None):
        p = self.params.reference
        x = np.squeeze(np.asarray(data.data, dtype=np.float64))
        if x.ndim != 2:
            raise ValueError(f"needs channels by time, got {list(np.shape(data.data))}")
        channels = x.shape[0]
        names = list(data.meta.get("channels", {}).get("dim0") or [])

        dropped = {n.strip() for n in p.exclude.split(",") if n.strip()}
        kept = np.array([i for i in range(channels) if (names[i] if i < len(names) else str(i)) not in dropped])
        if kept.size == 0:
            raise ValueError("`exclude` left no channels to build a reference from")

        mode = p.mode
        if mode == "none":
            out = x
        elif mode == "average":
            out = x - x[kept].mean(axis=0, keepdims=True)
        elif mode == "median":
            out = x - np.median(x[kept], axis=0, keepdims=True)
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
            if montage is None:
                raise ValueError("`laplacian` needs electrode positions on the `montage` input")
            pos = np.asarray(montage.data, dtype=np.float64)
            if pos.ndim != 2 or pos.shape[0] != channels:
                raise ValueError(f"montage is {list(pos.shape)}; it needs one row per channel")
            distance = np.linalg.norm(pos[:, None, :] - pos[None, :, :], axis=-1)
            np.fill_diagonal(distance, np.inf)
            near = np.argsort(distance, axis=1)[:, : int(p.neighbours)]
            out = x - x[near].mean(axis=1)

        meta = dict(data.meta)
        axes = dict(meta.get("channels", {}))
        if names:
            axes["dim0"] = names
        elif "dim0" in axes:
            del axes["dim0"]
        meta["channels"] = axes
        return out.astype(np.float32), meta
