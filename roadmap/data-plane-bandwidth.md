# `/data` bandwidth: depth, silence, and when compression pays

Decided 2026-09-16 from the performance audit, beside `viewer-render-surface.md`. The owner named
the `/data` stream as the first suspect for the frame-rate loss on complex patches. The
measurements say it is not the desktop's problem — the paint is — but it IS the tablet's, and the
same stream is what a LAN client draws from. This file is what the stream already does, what it
costs, and the order of the levers that remain.

## What the stream already does

- One reducer per watched `(node, slot)`, shared by every viewer of it, serving at most 30 fps
  and only a frame that changed — or the current one once, when a viewer joined, left or resized
  (`goofi-bridge/src/reducer.rs`).
- Every viewer declares a `ViewSpec` at its box in device pixels, quantized to 32 px
  (`viewers/capacity.ts`): a line asks for a min/max **envelope at its pixel width** and a
  subsample of rows, an image asks for an **area reduction to its box at 8 bits**, a viewer that
  declares nothing is served one texel. The bridge folds the specs of all viewers richest-per-dim.
- A producer that can make the frame at the demanded size does (`set_view_demand`); the reducer
  forwards an 8-bit frame that is already what the viewers asked for without decoding it.
- The browser decodes in a Worker and posts each frame to the main thread with its buffer
  transferred; `frames.ts` coalesces latest-wins and paints on one `rAF` under a 30 fps cap.

So the obvious reductions exist. What is left is the width of a sample, frames that say nothing
new, and per-message overhead.

## What was measured

Headless Chromium against a debug backend, 2026-09-16, idle after load:

| patch | viewers on screen | `/data` | `/control` |
|---|---|---|---|
| `thought-sphere` (35 nodes) | 31 line | 719 frames/s, **71 KB/s** | 125 msgs/s, 20 KB/s |
| `audio` (23 nodes) | 21 line | 480 frames/s, **1.55 MB/s** | 12 msgs/s, 1.7 KB/s |
| `auvis` (16 nodes, graphics + audio) | ~10 | 90 frames/s, 404 KB/s | 26 msgs/s, 3 KB/s |
| `music` (57 nodes), all viewers collapsed | 0 | 0 | 67 msgs/s, 6 KB/s |

- A line viewer at ~400 px wide costs ~3.2 KB a frame: an f32 envelope is `2 × width × 4` bytes.
  The audio patch's 1.55 MB/s (≈ 12 Mbit/s) is that, 21 times over at 23 fps — fine on
  localhost, most of a Wi-Fi link to a tablet.
- Decode is off the main thread and cheap. What the main thread pays per frame is the task and
  the structured clone of the `postMessage` (~0.13 ms), ×719 per second on `thought-sphere` —
  bounded by the backend's 30/s per slot, and gone once the render surface draws in the worker.
- A hidden editor tab drops `/data` to 0. Idle `doc_patch` was 0/s everywhere.
- `/control` is one JSON message per node per 500 ms for `node_stats`, plus `param_values` per
  driven node — 67 msgs/s on a 57-node patch with nothing visible. Small in bytes, not in count.

## Decisions

**Bandwidth is not the desktop frame-rate problem; measure the tablet before spending on it.**
The desktop's cost is the paint, owned by `viewer-render-surface.md`. The first step of this
track is the same measurement over the LAN to the tablet — frames/s and bytes/s per path, and the
tablet's own frame rate — so a lever is chosen against a number, not a suspicion.

**Done (2026-09-18): one latest-wins owner, no frame that says nothing new, half floats for
lines.** The worker decodes and posts on arrival; the tick, `latestRaw` and `DemandTicker` are
gone, and `frames.ts` coalesces under the paint cap alone. The reducer hashes the raw frame
(`goofi_codec::content_hash`) and ends the chain before the reduce when it says what the last one
served said, without taking a serve slot; a joiner, a leaver, a spec change or a re-offer is served
regardless. The hash skips the engine's per-emit stamps (`time`, `index`, `ufreq`) — every signal
node carries them, so a hash of the encoded bytes would never have matched. Accepted with it: the
metadata panel binds a null-spec viewer on the same stream, so a held frame's `time`, `index` and
`ufreq` freeze there while the node's rate still moves in the card header. `drift` is not skipped,
so an audio-clocked slot never suppresses. A recorder, a snapshot and a variable tap read the raw
frame as before. A producer's own ready-made (8-bit) frame is forwarded unchecked. `ViewSpec.depth`
is `f32 | f16 | u8`, folded to the widest admitted ask; a line viewer declares `f16`, the bridge
converts after the reduction — a frame with a finite sample beyond a half's range goes as f32 —
the wire spells it `<f2`, and the worker's decoder widens body and meta to f32 so nothing
downstream learns a second float width. `f16` was chosen over
`i16` with scale and offset: it is simpler, the frame needs no extra meta, and 11 significant bits
are more than a pixel resolves. Batching frames into one post per tick was rejected: it needs the
tick, and the dispatch it saves is bounded by the backend cap. The per-slot `/data` sockets stay.

**Generic compression comes last, and only shuffled LZ4.** WebSocket `permessage-deflate` is
rejected: floats compress ~1.3× under deflate and it costs the tablet CPU it does not have.
Byte-shuffle + LZ4 on frames above a few KB, negotiated by a `compress` flag in the `view`
message, is the one generic step worth having — after depth negotiation, and only if the tablet
measurement still asks for it.

**Not done: inferring overlap between frames.** A `Buffer` window re-sends ~1000 samples a frame
when ~33 are new, and a delta against the previous frame would cut that 30×. It is refused here
because "each frame counts in full, never infer sample overlap" is a rule of the whole data model,
and a transport that reasoned about sample identity would be a second owner of it. If it ever
comes, it comes as a producer-declared window step in the meta, not as a transport heuristic.

## `/control`, beside it

Not `/data`, but the same audit and the same fix shape: the 500 ms broadcaster sends one
`node_stats` message per node; one message carrying every node's rate is one parse and one
store write per period instead of N, and the frontend's per-event `nodeById` linear scan stops
being N² per period. Owned by `frontend-performance.md`, listed here so the two stream fixes land together.

## Order

1. The tablet measurement over LAN, recorded here.
2. Shuffled LZ4, if step 1 still asks for it.

## Open

- Whether a topomap's scalar per channel wants `i16` with scale and offset over the frame's own
  range: measure the visible difference on the surface before adding a second narrow depth.
