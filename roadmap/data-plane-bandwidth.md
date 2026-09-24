# `/data` bandwidth: the tablet measurement

The `/data` stream is not the desktop's frame-rate problem — the paint is — but it may be the
tablet's, and a LAN client draws from the same stream. What is left is chosen against a number,
not a suspicion.

## Remaining

1. **The tablet measurement over LAN.** Repeat the desktop baseline below against the tablet:
   frames/s and bytes/s per path, and the tablet's own frame rate. Record it here.

The desktop baseline (headless Chromium, debug backend, 2026-09-16) predates half floats and
frame silence, so the tablet numbers will not compare against it as is: re-take the desktop row
for the same patch on the same build first.

| patch | viewers on screen | `/data` | `/control` |
|---|---|---|---|
| `thought-sphere` (35 nodes) | 31 line | 719 frames/s, 71 KB/s | 125 msgs/s, 20 KB/s |
| `audio` (23 nodes) | 21 line | 480 frames/s, 1.55 MB/s | 12 msgs/s, 1.7 KB/s |
| `auvis` (16 nodes, graphics + audio) | ~10 | 90 frames/s, 404 KB/s | 26 msgs/s, 3 KB/s |
| `music` (57 nodes), all viewers collapsed | 0 | 0 | 67 msgs/s, 6 KB/s |

## Not to be done

**No generic compression.** Measured 2026-09-23 on 400 px envelope frames: byte-shuffled LZ4
buys 1.05× on EEG-like data and 1.4× on a sine, f16 or f32 alike; only a held constant compresses,
and frame silence already keeps that off the wire. `permessage-deflate` reaches the same ceiling
and costs the tablet CPU. Depth is the lever that pays, and it is taken.

**No inferring overlap between frames.** A `Buffer` window re-sends ~1000 samples a frame when
~33 are new, and a delta against the previous frame would cut that 30×. Refused: "each frame
counts in full, never infer sample overlap" is a rule of the whole data model, and a transport
that reasoned about sample identity would be a second owner of it. If it ever comes, it comes as a
producer-declared window step in the meta, not as a transport heuristic.

## Open

- Whether a topomap's scalar per channel wants `i16` with scale and offset over the frame's own
  range, where a line viewer's `f16` is relative to the value: measure the visible difference on
  the surface before adding a second narrow depth.
