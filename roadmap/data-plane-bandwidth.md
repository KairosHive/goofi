# `/data` bandwidth: what the tablet still pays, and when compression pays

Decided 2026-09-16 from the performance audit, beside `viewer-render-surface.md`. The `/data`
stream is not the desktop's frame-rate problem — the paint is — but it IS the tablet's, and a LAN
client draws from the same stream. The reducer already serves one changed frame per slot at
30 fps at most, reduced to the viewers' folded ask, at the narrowest depth the widest viewer
draws (`u8` for images, `f16` for lines). What remains is chosen against a number, not a suspicion.

## Baseline

Headless Chromium against a debug backend, 2026-09-16, before the depth and silence levers:

| patch | viewers on screen | `/data` | `/control` |
|---|---|---|---|
| `thought-sphere` (35 nodes) | 31 line | 719 frames/s, 71 KB/s | 125 msgs/s, 20 KB/s |
| `audio` (23 nodes) | 21 line | 480 frames/s, 1.55 MB/s | 12 msgs/s, 1.7 KB/s |
| `auvis` (16 nodes, graphics + audio) | ~10 | 90 frames/s, 404 KB/s | 26 msgs/s, 3 KB/s |
| `music` (57 nodes), all viewers collapsed | 0 | 0 | 67 msgs/s, 6 KB/s |

An f32 envelope at ~400 px is ~3.2 KB a frame; the audio patch's 1.55 MB/s is that 21 times over
at 23 fps, most of a Wi-Fi link. Half floats halve it.

## Decisions

**Measure the tablet before spending on it.** The next step is the baseline's measurement over
the LAN to the tablet — frames/s and bytes/s per path, and the tablet's own frame rate — recorded
here, so a lever is chosen against a number.

**Precision on the screen is the viewer's to lose, not the data's.** A line viewer draws `f16`:
11 significant bits are more than a pixel resolves, and a value no half holds goes as f32. A frame
whose range is small against its magnitude draws coarser than f32 would, and that is accepted:
the frame in the backend, the recorder, the snapshot and the variable tap stay f32 and exact.
`i16` with scale and offset was set aside for the same reason: simpler wins until a measurement
asks otherwise.

**A frame is two things on the wire, each sent when its own hash moved.** What a frame says —
body and descriptive meta, `drift` included, so an audio-clocked slot is never silent — and the
engine's per-emit stamps (`time`, `index`, `ufreq`). A held value goes once with its data and then
as a stamps frame per emit, which the browser folds into the frame it holds without a paint, so the
metadata panel keeps moving while the plot is left alone.

**Generic compression comes last, and only shuffled LZ4.** WebSocket `permessage-deflate` is
rejected: floats compress ~1.3× under deflate and it costs the tablet CPU it does not have.
Byte-shuffle + LZ4 on frames above a few KB, negotiated by a `compress` flag in the `view`
message, is the one generic step worth having — only if the tablet measurement asks for it.

**No inferring overlap between frames.** A `Buffer` window re-sends ~1000 samples a frame when
~33 are new, and a delta against the previous frame would cut that 30×. Refused: "each frame
counts in full, never infer sample overlap" is a rule of the whole data model, and a transport
that reasoned about sample identity would be a second owner of it. If it ever comes, it comes as a
producer-declared window step in the meta, not as a transport heuristic.

## Remaining

1. The tablet measurement over LAN, recorded here.
2. Shuffled LZ4, if step 1 still asks for it.

## Open

- Whether a topomap's scalar per channel wants `i16` with scale and offset over the frame's own
  range: measure the visible difference on the surface before adding a second narrow depth.
