# `/data` bandwidth: the tablet measurement

## Remaining

1. Re-take the desktop baseline (headless Chromium, debug backend: frames/s and bytes/s per
   path on `/data` and `/control`) on the current build; the 2026-09-16 row predates half floats
   and frame silence.
2. Repeat it against the tablet over LAN, with the tablet's own frame rate. What is done next is
   chosen against that number.

## Not to be done

- Generic compression: byte-shuffled LZ4 buys 1.05× on EEG-like envelopes and 1.4× on a sine,
  f16 or f32 alike; only a held constant compresses, and frame silence already keeps that off the
  wire. `permessage-deflate` reaches the same ceiling and costs the tablet CPU.
- Inferring overlap between frames: a delta against the previous `Buffer` window would cut it
  30×, but "each frame counts in full, never infer sample overlap" is a rule of the whole data
  model, and a transport that reasoned about sample identity would be a second owner of it. If it
  ever comes, it comes as a producer-declared window step in the meta.

## Open

- Whether a topomap's scalar per channel wants `i16` with scale and offset over the frame's own
  range, where a line viewer's `f16` is relative to the value: measure the visible difference on
  the surface before adding a second narrow depth.
