# Recording: every engine's frames, on one timeline

Any node's output, captured across every engine onto one timeline. Built 2026-09-07. `goofi-record`
owns the folder, the manifest and one writer per stream; each engine hands it frames through a door
of its own shape, and nothing else in the tree opens a recording file. What is left here is the
decisions — what the shape IS, and what was tried and rejected — and the few things still open.

## Time

**Time is only time.** It measures, and it decides nothing. Grid scheduling — a rate that is a grid
rather than a drift, and rhythm on top of it — is a LATER item and it is what will be called the
CLOCK. A clock measures time; it has no causal power over it, so the two must not share a name.

**A frame carries patch seconds, and the manifest carries the one UTC.** `Time::utc` is anchored
ONCE at the patch origin and advanced by the monotonic clock, so an NTP step cannot bend a
recording; `utc_at` dates any patch second against that anchor. A reader adds the two. Wall time per
frame was the alternative, and it makes every frame a place the step can lie.

**The stamp is the RUNTIME's, never the node's.** `stamp_meta` runs after `process()` and is the one
stamping site. A node author writes nothing and cannot break alignment — which is also what keeps
the subprocess tier correct, since its `Instant` has a different origin and it must never mint one.
A node that KNOWS a device time — LSL carries one — puts that in `Meta` itself, as a convenience
entry for later analysis. goofi time stays what everything aligns on.

**Sample times are DERIVED, never stored.** `sfreq` and the shape are already in `Meta`, so sample 0
of a 256-sample window at 256 Hz emitted at t sits at t − 1. A `Buffer` is correct for free. Where a
derived stream's provenance matters, the answer is to record its source too and measure the offset
from the two recordings.

**A rate-locked stream derives its timeline from the SAMPLE COUNT, and the manifest says which it
used.** For audio the count IS the clock and it is exact; a clock read per block adds scheduling
jitter to a timeline that had none. A self-paced signal node has no counter, so its tick read is its
timeline. `measured` and `derived` are the ENGINE's word rather than a guess off `sfreq` — a rate
alone cannot say whether the times were counted or read — because a derived timeline and a measured
one are not the same evidence.

**The audio anchor is tied under the RUNTIME LOCK, and the audio thread reads no clock.** `retune`
holds that lock, so no block is rendered between reading the count and reading the clock, and every
block after is a function of its number alone. Reading the clock at the drain instead was built
first: the differences were exact, so the stream was internally consistent and up to one control
tick — 10 ms — late against every EEG stream it exists to be lined up with, with nothing in the file
to say the anchor had been guessed.

**The tie is taken ONCE, so the manifest carries the drift.** A derived timeline counts the device's
samples and patch time is the monotonic clock; at a routine 100 ppm the two walk about 360 ms apart
per hour. Re-tying periodically was the alternative and it was rejected: it puts a seam in the exact
block spacing that makes the timeline derived at all. So the engine MEASURES the gap instead, the
frame carries it in `Meta`, and each stream's manifest entry holds the last word as `drift` —
seconds the stream stands ahead of patch time, which an analyst subtracts.

## The recording

**A recording is a DIRECTORY, not an archive.** Unlike a `.gfi` it is written incrementally and can
be enormous, so packing is a later action rather than the format. The folder is named for the UTC it
began at; `manifest.json` sits beside one file per stream, named
`<node>-<slot>__<UTC of its first sample>`. The root is `globals.record.root`, else
`~/.goofi/recordings`.

**A stream file is the format its own SHAPE takes, and nothing writes the wire format to disk.**
An array is a `.npy`, a table is a `.csv`, text is its own lines, audio is a `.wav` of IEEE float32
and a texture is a video — each chosen because it IS that type's shape, which is more orthogonal
than forcing five kinds through one container. Every one is append-only behind a fixed-size head
that is patched on the sync cadence, so a writer that is killed costs the tail and nothing else:
the whole frames follow from the file's size, and `np.load` on a truncated `.npy` fails honestly
rather than reading nonsense. NPY has ONE spelling in the tree, `goofi-record`'s, which the
`/data` plane's `--raw` reply uses too.

**An array's `.npy` is FLAT, and the sidecar is what splits it.** The header holds one 1-D shape —
every value the stream ever carried, in order — and a frame of any shape appends into it untouched.
A stack of frames was there first, its own first axis the count, and it could hold ONE shape: a
`Buffer` filling its window grew by a frame a tick, so recording one at `size` 1000 minted about a
thousand files. Which is the general case rather than that node's quirk — a peak count moves with
the peaks, and a shape is a thing a node is entitled to change. So a reshape is no longer a reason
to open a file, `Kind::Array` carries no shape at all, and `Stream::takes` answers true for every
array frame through the arm it already had. What is given up is the ONE `np.load` that returned
`(frames, …)`: a uniform stream folds back with the manifest's `frame` in one line, and a ragged
one is split by the sidecar, which a reader opens for the instants anyway.

**Every stream has the same sidecar: one JSON line per frame.** `{"t", <extent>, "meta"}` — the
instant, what the frame takes OF THE FILE, and the `Meta` the file itself cannot hold. The extent is
what makes it an INDEX rather than a note: no file here holds frames of one fixed size, so without
it nothing can say which samples belong to which instant. A line is also the unit a kill truncates
to. The meta is serialized STRAIGHT to the file — building a `serde_json::Value` per frame cost
4.3 µs a line against 0.33, and a fast stream outran its own drain.

**The extent is a SHAPE or a row count, and never both.** An array says `"shape"`, and only on the
line where it MOVED, because a reader carries the last one forward; every other kind says `"n"`,
its own units, because it has no shape to carry — a `.wav` holds blocks of no fixed length and a
`.csv` row is a row. Both on one line would state one fact twice, since a count is `prod(shape)`,
and `Extent` is the type that makes writing each impossible. `Shapes` on the stream is the one
owner: it takes each frame's shape, answers it back only where it moved, and the manifest's `frame`
is its projection — the shape while every frame agreed, absent once one did not. A uniform 200 Hz
`[1, 4]` stream's line is now `{"t":…}` alone, which is SHORTER than the `"n":1` it replaced.

**A frame that no longer fits opens the NEXT file**, which is the rule a resized texture already
followed: a table with other columns, a `.wav` at the 4 GB ceiling RIFF counts in. An array is no
longer among them. RF64 lifts that ceiling and is read by far less than plain WAV, which is the whole reason
a recording is a WAV at all.

**The manifest is a PROJECTION, minted at every rewrite** from the closed streams' entries and the
open streams' own state, then written beside and renamed. No count lives both on a stream and in a
record of it.

**A re-arm mints a new file.** `free_name` takes the first name the folder does not hold and the file
is opened with `create_new`, so truncation is not a thing the type can do. It cost a take: a disarm
and a re-arm at the same UTC instant re-opened the name just closed and wrote over the eight frames
already in it.

**A rebirth is a gap, and it is recorded.** `service_base` carries `gen`, bumped on every birth, so a
param change that restarts a node mints a name the drain has never opened. It closes the old stream
as `reborn` and opens a new file. A recording that smooths over a real gap is worse than one that
stops.

**A real-time node is never stalled: every engine drops and counts.** The requirement is "never
SILENTLY", not "never" — a disk that cannot keep up loses data in any design, and the recording says
where. A drop is a count and an instant in the manifest, and a standing error where the engine has a
fault path to raise one on.

**A recording is SINGULAR: one patch, one session.** A `session load` ends it, because a load swaps
the graph whole and mints new service names, and a recording that survived one would hold two
patches in one folder.

## Arming

**Arming is the NODE'S OWN RECORD** — `doc.nodes[<uid hex>].record`, the armed output slots as a
`string[]`, always present. So it is undoable, saved in the `.gfi`, copied and pasted with the node,
and restored by a load, with no second holder anywhere. It reaches the engines only through
`settle`, as every other settled fact does: `Touched::Record` on the write, `NodeView::recorded` on
the read.

**The plan owns the recorder's bell.** `wire_out` replaces an output's target set WHOLE, so a bell
the recorder opened for itself would be disarmed by the next re-wire — a cable three nodes away
stopping the recording, with no error anywhere.

**Nothing keeps a second armed set.** Each holder of one cost a defect: the signal drain's
`Feed::opened` mirror could only ever clear and never restore, and the graphics runtime believed its
own tape, so a disarm and a re-arm between two ticks left it writing at a stream the recorder had
closed and no new file ever opened. `Recorder::is_open` is the owner, and it is asked.

**The panel arms by drag, and the drag raises an op.** `record arm <node>/<slot>` like every other
intent, so the CLI, an agent and a test reach the same door. The drop is a generic
`UIStore.onNodeDrop` registration rather than a branch on panel type. Whether a recording runs is
runtime and not document: `record status` reads it, and a backend beat broadcasts `record_changed`
once a second while one runs, so the header's indicator counts up with the browser polling nothing.

## The three engines

Recording is a door on each engine and one central recorder behind them. The three do not share a
problem.

**Signal publishes the same encoded bytes twice.** An armed slot opens a dedicated iceoryx2 service
and `publish` sends to both. The budget is STATED and FIXED: `RECORD_BUDGET` is 64 MiB for an armed
slot, cut by `record_shape(engine)` into 1 MiB × 64 for signal and 64 KiB × 1024 for audio, with
`AllocationStrategy::Static`, so an outsized frame is refused rather than growing the segment.
Raising the shared data plane's depth was the alternative and it MULTIPLIES: depth is a
service-level property, so 256 subscribers of a 64 KB chunk make a depth of 32 half a gigabyte a
slot. Never the `/data` plane, which is lossy by design at every hop.

**The service overflows safely, so `Meta::index` is the one witness of a loss.** The oldest frame is
dropped at the SUBSCRIBER, which the publisher's own counter cannot see, so the drain counts the
gaps in the indices instead. An index that RESETS is a rebirth and is never counted as a loss.

**One door, and the event id is ignored.** The recorder holds a single event service; any producer
rings it and the drain sweeps EVERY armed feed on wake. That spends none of the `EventId` budget,
and a burst of rings coalesces into one sweep.

**Audio writes a PRE-ALLOCATED lock-free ring on the callback thread**, one per output and beside
the tap — no allocation, no lock, no syscall — drained on the control half, which publishes onto the
recorder's own service as the signal engine does. So the audio engine holds no recorder of its own.
A block's number is the AUDIO thread's: a drop counter read at the drain and applied to blocks
already queued was what it cost, because the index run then stayed contiguous, so the loss was
invisible and every surviving block was dated 1.33 ms late.

**Graphics is a third `Want`, and arming registers demand.** A stage renders only where its output
has a reader, so an armed stage is one — at its OWN size, which is the stage's and never somebody's
viewport. Frames leave through an `Encoders` seam; the one implementation
is an ffmpeg child writing FFV1 in Matroska, `rgba64le` in and `gbrp10le` out, beside a `.times`
sidecar of one f64 patch-second per ENCODED frame. Encoding runs off the render thread behind a
two-deep queue, and a frame the queue refuses is a counted drop rather than a stalled engine.

**A video is what a viewer WATCHES, and the exact texels are not in it.** No integer format holds
an `Rgba16Float` texture — the 16-bit unsigned mapping that was there first is finer than f16 above
1/64 and COARSER below it, and anything under 7.6e-6 becomes 0, so "lossless within [0,1]" was an
overclaim at the dark end whatever the depth. So the video is stated as a viewable PROJECTION and
claims no losslessness at all: ten-bit `gbrp10le`, which is 222 KB a frame at 512 square against
797 for sixteen — 6.5 MB/s at 30 fps instead of 23. An analyst who needs the texels records the
tap, which is f32.

**It carries three channels, because a fourth is a file that plays as nothing.** The readback is
RGBA and the recorded stream is not: VLC cannot allocate a picture for a 16-bit ALPHA plane, so
`gbrap16le` reaches the decoder and dies there — `get_buffer() failed`, then `buffer deadlock
prevented`, and a window that stays empty. The same frames as `gbrp16le` play, and so do
`yuv444p16le` and 8-bit `gbrap`: it is depth AND alpha together that no common player takes. YUV at
any depth was rejected beside them — its matrix is a lossy conversion the RGB path does not pay.
The manifest says in words that alpha is not kept.

**The container is CONSTANT-RATE, so the encoder must hold the render clock.** A rawvideo pipe
carries no timestamps — `-use_wallclock_as_timestamps` with `-fps_mode passthrough` was tried and
the demuxer ignores it — so the file's timeline is `-r` times its frame count, and every dropped
frame shortens the recording rather than gapping it. What that cost: FFV1's default is ONE thread,
19 frames a second at 512 square, against a clock that then rendered 60 — two frames in three
dropped, and three seconds of patch time playing back as one. Slicing it (`-slices 24 -coder 1
-context 1`) reaches 73 fps on the same frames and files 9% smaller than slicing alone, and the
render clock is 30. The `.times` sidecar stays the alignment authority, which is what makes a drop
recoverable rather than silent.

**Finalizing never runs on the render thread.** `Recorder::close_later` reaps on a thread of its own
for every close the render thread causes. Closing in place held the session mutex across
`finish` → join → `child.wait()`, so a disarm or a resize of one video stalled every window, every
viewer and both other engines' drains until ffmpeg had finished; and a manifest rewrite under that
same lock parked the render thread — and with it the one process-wide GPU gate — on a disk write.

## What the drain and the sidecar cost, measured 2026-09-08

**Formatting left the drain, one LANE per stream.** Writing was a memcpy of the wire bytes; it
became a `.npy` append plus a JSON line, and the one drain thread that serves every feed fell
behind a 100 kHz one. The shape was the answer rather than the constant, and the video path had
had it all along: a frame is copied into a pooled buffer, queued, and formatted on a thread of the
stream's OWN — because a shared queue makes one stream's overload another's loss. A full lane is a
counted drop, never a stall, witnessed by the same index gap a loss at the subscriber is.

**A failed write is the stream's death, said once.** The move to a writer thread had left the
drain's `failed` flag unwritten, so a full disk retried the open per frame and lost every frame
after it in silence — `close_now` is the writer's own door, as `open_now` is, and it files the
reason in the manifest.

**The manifest is a projection, and it lands on the STREAMS' own cadence.** A rewrite is a create,
an fsync and a rename, and it was one per sweep — up to fifty a second, on the drain thread, which
is the thread that must not fall behind a transport. Nothing is fresher for it: `record status`
reads live state, and every count already rides its own write.

**The sidecar said the instant twice and the stream's constants every frame.** A line was
`{"t",…,"meta":{sfreq,ufreq,time,index,reduced:null}}` — 122 bytes for a four-float frame, where
`time` duplicated `t`, `sfreq` duplicated the manifest, and `reduced` was always null. A line now
carries only what MOVED, a reader carries the rest forward, and `t` is the one owner of the
instant. Measured over 5 s: a 200 Hz `[1, 4]` stream's sidecar went from 740% of its data to 443%,
a 2 kHz one from 94% to 57%, and the recorder's whole cost at 2 kHz from 18.2% of a core to 11.4%.

**Nothing is lost, and every loss is counted, at every rate measured.** 200 Hz, 250 Hz at 64x256,
and 2 kHz all recorded with zero unaccounted index gaps. Against a manufactured audio overrun of
256 seconds in one render call, the manifest's `dropped` equalled the missing block numbers
exactly on all eight runs, at losses from 25% to 98%.

## What was decided against

**A ragged container was measured against the flat `.npy`, and every one lost.** The bar is the
recorder's own two: the format must APPEND, so a killed writer costs the tail alone, and a tool the
analyst already has must open it. Measured 2026-09-08.

- **A zip of one `.npy` per frame (`.npz`)** writes its directory at the END, so a killed writer
  leaves a file `zipfile` refuses whole. Rewriting the directory each sync fixes that and grows
  with the frame count, so the rewrite slows forever.
- **Concatenated NPY** — one complete `.npy` per frame, one after the other — passes both bars:
  four frames of `(3,3)`, `(5,3)`, `(2,3)`, `(7,3)` read back exactly through a loop over
  `np.load(handle)`, and a file cut short returned every whole frame and then raised. It costs 128
  bytes of header PER FRAME, which on a 200 Hz `[1, 4]` stream is eight times the data, and
  `np.load(path)` on it returns frame 0 SILENTLY — a worse failure than a file that does not open.
- **Arrow IPC**, in the STREAM spelling, is crash-safe and was rejected on cost: the missing
  end-of-stream marker did not trouble pyarrow and a cut mid-batch read every batch before it. It
  costs 315 bytes a frame, FIXED whatever the frame holds — 1967% of a `[1, 4]` frame, 0.5% of a
  `[64, 256]` one — plus 13 crates new to the lockfile, a copy per frame to build the array and its
  offsets, and a read side that is pyarrow rather than numpy. The Arrow FILE spelling and Parquet
  both write an index at the end and fail the first bar outright.
- **HDF5 and netCDF** need libhdf5 through bindgen, and a killed file can be unreadable whole
  rather than in the tail. **Zarr** makes one file per chunk, so a ragged stream makes thousands.
  **Padding every frame to a maximum shape** writes values the node never made.

**Only the leading axis folding into the index was rejected too**, and it is the one that would
have kept `np.load` returning a stacked array. It needs the varying axis to be axis 0, and here it
is usually the LAST — a `Buffer` defaults to `axis -1`, and axis 0 is channels — so it would fold
the CHANNEL axis of a `[64, 256]` stream, recoverably and wrongly, to serve the ragged case.

**Compressing a stream file is not worth what it costs.** Measured 2026-09-08 on 26 MB of
realistic f32 biosignal content — a random walk at microvolt scale, which is what a stream
actually holds: `zstd -1` 1.19x, `zstd -9` 1.20x, `gzip -1` 1.26x. The property given up is the
whole point of the format — a `.npy` is what `np.load` opens, and a compressed one is what a tool
goofi ships opens. A synthetic ramp compresses 7500x, which is why the measurement had to be on
content and not on a fixture. The disk lever was never the container: it is the sidecar, above.

**A recording is LOSSLESS raw and nothing else.** WAV, MP4 and CSV are each lossy against a `Data`
frame: WAV loses anything not audio-shaped, CSV loses f32 below nine digits, MP4 by construction.
They are CONVERSIONS of the written file, made later and by a separate tool; nothing is written
beside the recording, and no format choice can undo the buffered path.

**Graphics is the stated exception, and it is stated in the manifest.** Every texture is
`Rgba16Float` and no integer format holds one, so a video entry says in words that it is a viewable
projection and not evidence: ten bits of red, green and blue, a value outside [0,1] clipped to it,
and alpha not kept. It claims no losslessness at all, plain or qualified.

**Video is encoded through ffmpeg, and the owner chose it against measured alternatives.**
In-process pure-Rust compressors were offered with numbers, and ffmpeg was kept. The numbers stand
for whoever proposes one again: single-core, on a 1920×1080 RGBA16 frame of GRADIENT content,
`lz4_flex` reached 735 MB/s at 2.0x and `zstd -1` 451 MB/s at 7.9x — both faster than FFV1, and
zstd denser. Gradient content makes those ratios optimistic against real shader output. The trade,
as a fact about the two formats: a `.mkv` opens in any player, and a goofi frame stream opens in
goofi. That first half is a fact about the CONTAINER and not about what is put in it — a `.mkv` of
`gbrap16le` opened in nothing — which is why the pixel format is a decision and not a default.
The dependency is bounded — a missing ffmpeg costs THAT STREAM alone: the node wears a
standing error naming the package, the manifest holds an entry saying why the stream is empty, and
only a recording whose every armed stream is video is refused.

**The audio engine's own take recorder is deleted.** The `record.*` params on `AudioOut`, `Rec`,
`wav::Writer`, `take_stem` and `part_path` are gone; `wav::Reader` stays, for playback. Two
recorders cannot own one timeline.

## Open

- **Packing.** A recording is a directory; making one file of it is an action, and it has no op.
- **The panel add-on loader.** The recorder panel is compiled in, as `library.md`'s first tenant
  says; the loader itself is that file's item.
- **An audio stream has no channel labels**, so the manifest's `channels` is null for one and the
  count is in every frame's shape.
- **The first frames of a stream are lost UNCOUNTED, and this is the one hole left in "never
  silently".** The producer opens its record publisher at the arm; the drain opens its subscriber
  on the first `resolve` after `record start`, and iceoryx2 has no history, so whatever was
  published in between is gone. The gap cannot be counted either: `feed.last` is `None` on the
  first frame, so the first index is taken as the beginning whatever it is. It is a handful of
  frames — the producer's own publish rings the drain's door — but it is the only loss in the
  system that leaves no trace. The fix that closes it is holding the subscriber for as long as the
  slot is ARMED rather than only while a recording runs, which costs a drain-and-discard on an
  armed-but-idle slot; whether that is the right trade at 100 kHz is the owner's call. The
  cheaper half — the drain filing the first index it saw, so a reader can see where the file
  begins — closes nothing but makes it visible.
- **A frame costs three string clones to reach its lane.** `Writer::take` clones the whole
  `StreamId` — node, slot, engine — to key the lane map and again to ride with the queued frame,
  which is six allocations a frame at whatever rate the producer runs. A lane INDEX handed back at
  the first `take` would cost none. Nothing measures it as the dominant term yet; the recorder is
  11.4% of a core at 2 kHz and the formatting is most of that.
- **`ufreq` rides every sidecar line and is derived from the column beside it.** It is the node's
  own EMA of its update rate, seventeen significant digits of it, and the sidecar's `t` column IS
  the emission times it is measured from. It survived the delta above because it moves every
  frame, and it is what is left of a line: about a third of one, and the difference between a
  200 Hz stream's sidecar being 443% of its data and about 300%. Those two figures are from before
  `"n"` left an array line, so both now stand lower by its width. Dropping `ufreq` means a reader
  recomputes the rate from `t`, which is not the same number as the engine's EMA.
- **A rate change mid-recording** re-ties the audio anchor, so the frames either side of it derive
  from different ties. That is a real discontinuity and the manifest does not name it as one — only
  the `drift` either side of it moves.
- **A stage the ENGINE cannot render at 30 fps still plays fast.** The container is constant-rate,
  so a heavy shader or a huge frame that overruns the tick shortens the video the same way a slow
  encoder does — the drop just happens one stage earlier, and nothing in the file says so. The
  `.times` sidecar holds the truth either way. Making the file itself honest needs per-frame
  timestamps, and a rawvideo pipe carries none: that means a container goofi writes, or a retiming
  pass over the finished file.
- **A reaper that races a stop loses that stream's manifest row.** The file is still finalized, so
  the recording keeps the video and loses only what describes it. The fix was built and WITHDRAWN:
  making `stop` wait for `close_later` parks it inside a blocking ffmpeg `finish`, which is the
  stall `close_later` exists to prevent. A rare lost row is the better trade until the row can be
  filed without the wait.
