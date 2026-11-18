# 02. Foundation

The foundation is the part of the goofi layer that every other part stands on. No module of the
foundation knows a graph, a node, an engine or an application.

## 2.1 Time

- One module owns time: the patch clock that every stamp reads, and every way anything waits,
  paces or measures. Nothing else sleeps in a loop, sets a socket timeout, rounds a deadline to a
  tick or keeps a clock of its own.
- The patch clock gives: the patch time in seconds, the wall time, the UTC time of a patch
  instant, and a restart.
- The patch clock has three sources: the wall clock (live mode), data time (Kahn mode, part 06),
  and an external driver (a test harness that advances time by hand).
- Waiting primitives: an event with ids that says which source rang; a deadline; a pace (a
  period that does not catch up after an overrun); a grid (instants on a fixed lattice); a rate
  meter; a sleep until an instant; a wait for a readable descriptor.
- Every wait has a ceiling, and the ceiling is a value the owner states. The one exception is the
  stop of a recording, which waits until every file is closed.

## 2.2 Paths and tools

- One module owns every path and every tool lookup (part 01.5).
- The home is `$GOOFI_HOME/.goofi`, by default `~/.goofi`. It holds: `config.toml`, the list of
  recent folders, `library/` (the installed bundles and the private bundle `_local`),
  `recordings/`, `datasets/`, `recovery/`, `plugins/`, `plugin-data/`, and the account
  credentials.
- The runtime is `$GOOFI_RUNTIME`, by default under the data directory of the platform. Per
  version it holds the two Python environments, the build tree and the shipped node artifacts.
  A shared cache holds downloads.
- The session directory is in the temporary directory of the system, one per running program.
- Tools found on PATH: `cargo`, `rustc`, `uv`, `npm`. A missing tool is reported with the action
  that needs it.
- Every cache has a content key (never a date), one owner and one pruning rule. A built artifact
  is immutable. A file is written whole or not at all (written beside, then renamed).
- A runtime tree of another version is removed only when no live session uses it.

## 2.3 Session and resources

- A session owns every ephemeral resource of one running program. Its lock is the one answer to
  "is it alive". A dead session is swept by the next program that starts.
- Every child process, long-lived thread, scratch path and device is minted through the session
  and listed by `session status` until it ends.
- A child gets the session in its environment, a liveness pipe to its parent and a process group
  of its own. Its output goes to the log unless its owner wires a pipe or a file. A child ends
  itself when its parent dies. To stop a child is to ask, wait a grace time, then kill.
- The session sees the death of a child and tells its owner. No owner learns of a death by a
  failed call.
- A thread is named and listed while it runs. A join has a ceiling.
- Shutdown has one release order, and the session executes it.
- Every engine and service reports its status through one status shape, shown by
  `session status`: its clock, its devices, its counters and the cost per node.

## 2.4 Building components

- A component that takes time to build (the instantiation and setup of a node, the pipeline of a
  shader, the open of a device, the birth of a plugin) is prepared on a thread of its own.
- The system keeps running on what it has. The owner takes the finished component at its next
  settle or tick and swaps it in whole.
- A tick never waits on a build and never sees a half-built component.
- A build that fails or panics is a refusal that the owner reports.

## 2.5 Log and configuration

- One log serves the program. A row is a group: one component, node, level, stream and exact
  text. A repeat raises the count of its group and moves it to the newest position.
- Limits: 10 000 groups, 16 MiB of text, 64 KiB per message.
- The standard output and error of children and of Python nodes are captured into the log with
  the node as source. A traceback or an error line is an error; other error-stream text is a
  warning.
- A reconnecting client gets the retained groups; a live repeat sends only its identity, count
  and last time.
- Ops read the log and write a message with a level and a component. The log cannot be cleared.
- `config.toml` in the home holds the configuration a person edits: the agent harness entries
  and the storage locations. A config that does not parse falls back to the defaults and reports
  why.

## 2.6 Payload

### 2.6.1 Data

- A frame is an immutable value with metadata. Frames are shared by reference inside a process.
  Equality is by value and ignores the stamps.
- Value kinds:
  - **array**: a shape and samples. Time is the last axis; channels are the axis before it.
  - **string**.
  - **table**: an ordered map from names to frames; tables nest.
  - **texture**: pixels (width, height, format: 8-bit or float, RGB or RGBA, top row first,
    straight alpha), at most 8192 per side.
  - **functional**: a value as a function of patch time, evaluated by whoever consumes it.
  - **event**: something that happened once at an instant, with an identity and an optional
    payload.
- A zero-dimensional array becomes shape `[1]`.
- The meta of a frame is an ordered map with string keys. Value types: null, boolean, integer,
  float, string, list, map, bytes.
- Built-in meta keys: `sfreq` (sampling frequency of the last axis), `channels` (labels per
  dimension, `dimN: [...]`; the count of labels equals the length of the dimension), `reduced`
  (what a viewer reduction removed), `drift`, and the stamps.
- Helpers keep `channels` correct when an axis is dropped, selected, concatenated or inserted.

### 2.6.2 Stamps

- The runtime stamps every frame a run emits: `procstart` (the start of the run), `procend`
  (read after the run), `index` (the emit count of the node; a gap is a lost frame), `ufreq`
  (the smoothed update rate, absent until the second emit), `node`, `slot`, and `source`.
- `source` names, per input slot, the frame held there, by its `node`, `slot`, `index`,
  `procstart` and `procend`. It holds the
  immediate inputs only, never the history behind them.
- Stamps are written at one site and are never inherited.
- Each frame counts in full, including a window of a buffer. Sample overlap between frames is
  never inferred.

### 2.6.3 Value descriptions

- One description of a typed value with a range serves node params, variables, control widgets
  and machine attributes: kind (number, boolean, string, pulse), dimensions, minimum, maximum,
  integer, options, and face (plain, colour, envelope).
- Coercion is one rule set: a number fills or cuts dimensions; one number fills all dimensions;
  an integer rounds; truth and text have one reading each.

### 2.6.4 Shared codecs and tables

- Each file format has one reader and one writer in the goofi layer: NPY, WAV, CSV, the take
  manifest and its sidecars.
- Each shared table exists once and is generated for every language that needs it: the 10-20
  electrode layout, the geometry format, the easing curves, the colormaps, the name rules, the
  node metrics of the canvas.

## 2.7 Wire format

- One binary frame format carries a frame between processes and to the browser: a magic, a kind
  tag, the meta as a compact map, and a body aligned for a zero-copy view of the samples.
- Viewer variants carry narrow depths (8-bit, half float) and a stamps-only frame.
- A content hash of a frame omits the stamps.
- Decoding checks every bound and limits nesting depth.
- A golden file holds reference frames. The backend test and the frontend test read the same
  file.
- Constants of the format are generated for the frontend from the backend source.

## 2.8 Transport

### 2.8.1 One API

- The transport is one standalone layer with one API through which two ends exchange frames. The
  API is the same in process and across processes.
- The case is detected, not chosen: in one process the frame is handed over by reference with no
  serialization; across processes it is serialized through shared memory.
- An end is found by a name. The application derives the names; the transport does not know a
  graph.
- Channels: in process, shared memory, WebSocket.
- Every packet holds a frame with its meta.
- The transport has Python bindings. A Python subprocess node sends its packets itself, peer to
  peer, to other nodes, to the recorder and to the browser data plane.
- There is no request and reply. A node works by producing. Control messages to a hosted node
  (settle, param edit, pulse, refresh, stop) and its status and errors are packets on queued
  connections of the same transport.

### 2.8.2 Delivery

A connection has one of two deliveries:

- **latest**: one cell that holds the newest frame. A reader takes each frame at most once. A
  late reader of a held cell gets the newest frame. The count of readers is a demand signal for
  the producer.
- **queued**: every frame in order. A queue has a bound and a policy at the bound:
  - *wait*: the sender parks until there is room. A queue of this policy also carries the
    progress of its producer (part 06.3.3).
  - *drop oldest*: the sender never waits; each drop is counted and shows as a gap in the frame
    index.
  - *no bound*: the receiver queues every frame. The receiver states the memory limit at which
    it reports and what it does then.

| Use | Delivery |
| --- | --- |
| A data cable in the live mode | latest |
| An event cable in the live mode | queued, drop oldest |
| Every cable in the Kahn mode | queued, wait |
| A stream to the recorder | queued, no bound |
| A stream to a viewer | latest |
| Control and status of a hosted node | queued |

- A node waits on one wake-up primitive that says which of its inputs rang.
- The transport stops within a ceiling at shutdown.

## 2.9 Recorder

### 2.9.1 Function

- The recorder exists for robust recording of data. It never drops a frame.
- It is a standalone component: a program runs it as a thread, or as a process of its own, with
  the same interface.
- Each program holds its own recorder.
- Engines publish frames and never touch a file.

### 2.9.2 Interface

- The interface does three things: it sets the root folder, it sets the recording session (a
  folder inside the root that holds the takes of one sitting), and it registers streams.
- A registered stream is a named, queued receiver that the recorder holds. A producer only
  pushes.
- The recorder takes frames from the threads of its program by reference, and from other
  processes through the transport.
- To arm an output of a node registers a stream. The armed set is part of the patch (part 03.2)
  and is applied after every settle.

### 2.9.3 Takes

- A take starts with a root, a name, the patch and annotations. A start is refused while a take
  runs, when nothing is armed, or when the name holds a path character.
- Annotations are an object by namespace, written before capture starts.
- Stop sets the boundary, makes every engine flush, waits until every stream has closed its
  file, then writes the final manifest.
- A stream armed during a take opens at once. A stream disarmed during a take writes what it
  holds and closes.
- Timelines: a stream is *measured* (signal, graphics: a frame is in the take when its
  `procstart` lies between the boundaries) or *derived* (audio: a block is in the take by its
  block number). The manifest states the timeline and the drift of each stream.

### 2.9.4 Recorder types

A recorder type puts frames into a file with its own buffering and flushing. A modality does not
know the recorder type.

- **Files**: one native file per stream, chosen by the kind of the frame.
  - array → flat NPY (float32; a killed writer leaves every whole value readable);
  - table → CSV (a time column and the columns of the first frame; new columns open a new file);
  - string → text, one line per frame;
  - audio → WAV, float32 interleaved, a new file at the size limit of the format;
  - texture → video, with a quality of `small`, `high` or `very_high`; a resize or a quality
    change opens a new file.
  - Each stream has a sidecar with one line per frame: the patch time, the shape when it
    changed, and the meta as a delta.
  - One manifest per take: the program version, the name, the patch, the UTC instants, and per
    stream its file, node, slot, engine, start instants, rate, timeline, drift, channels, frame
    count, losses and the reason it closed. A stream that could not open is an entry with its
    error.
  - Take folder: `<UTC stamp>Z[-name]`. Stream file: `<node>-<slot>__<UTC nanos>Z.<ext>`. A file
    is never overwritten.
- **Dataset**: the recorder writes a dataset file directly (part 10.1): one blob per
  stream, one sample for the take, paired and synchronized, with tags and annotations.
- A functional frame is not recorded.

### 2.9.5 Status

- Status: running, folder, start instant, and per stream its file, frames written, queue fill
  and error. A change of status is pushed to clients.

## 2.10 The browser data plane

- A viewer in a browser subscribes to one output slot of a node. One stream serves every viewer
  of a slot, with one reduction for all.
- A viewer declares what it can draw: the kinds and dimensions it accepts, a cap per dimension
  or "whole", a depth, an aspect rule, and its display rate.
- The server merges the declarations per frame: per dimension the largest cap wins; "whole"
  stops reduction of that dimension; the widest depth wins. A reader that declares nothing gets
  a bounded preview.
- Reduction keeps what a person sees: a line is reduced by minimum and maximum per pixel column,
  an image by area average. `meta.reduced` records the original length and coordinates.
- A viewer never receives more than it can draw. The budget is in device pixels.
- Rate: a stream is served at the lowest of the display rate of the viewer and the viewer rate
  of the patch (a variable, 30 per second by default). An unchanged frame is not sent again; a
  frame whose stamps alone changed sends its stamps alone.
- A functional is evaluated at the patch time of each serve.
- Demand reaches an engine as a flag and a wanted size. It never changes a plan or the schedule
  of an engine. A producer that renders for a viewer alone (a graphics stage) renders at the
  wanted size.
- A subscription exists only while its viewer is in the viewport.
- A slow client drops frames and never stalls the stream of another client.
- A snapshot op reads the newest frame of a slot at full resolution.

## 2.11 Pitch and scale

- Pitch is Hz on the signal plane and volts per octave on the audio plane; 0 V is C4.
- One module owns scales: the presets (chromatic, major, minor, the modes, pentatonic, blues,
  harmonic minor, whole tone, harmonic), custom scales by generator, harmonics or division, and
  the snap of a pitch to a scale. It owns the conversion between Hz and volts.

## 2.12 Open decisions

1. **The dtype of an array.** Every array frame of the present program is float32, and other
   types are cast on entry. A dataset blob has any dtype. Decide whether a frame carries its
   dtype (with float32 as the default and a cast at the input of a node that accepts float32
   only), so a recipe keeps integers and float64 exact.
2. What replaces ffmpeg for video.
3. The memory limit of an unbounded recorder queue and what the recorder does at it.
4. The channel of the recorder interface when the recorder is a process of its own.
5. The unbounded queued connection across processes in shared memory.
6. Which recorder type is the default of goofi-pipe.
