# goofi-data

goofi-data is a platform of its own for datasets: it keeps, labels, derives, records and shares
them. It is an offline tool and a management system: a full data platform. Status: planning;
nothing is built. This file records the product decisions of 2026-11-06, 2026-11-07, 2026-11-11
and 2026-11-12. `goofi-build.md` holds the model builder.

goofi-pipe records; goofi-data keeps, labels, derives and shares what was recorded; goofi-build
makes models from it. With the online node library, the three form one ecosystem: a meeting point
and a marketplace for autopoietic research and arts.

goofi-data has three cornerstones. A cornerstone is a core function that constitutes the software
and is composed with the other cornerstones:

- **Storage**: datasets, their layout, their adapters, and exchange between places and people.
- **Recipes**: derivation as a graph of stations and queues, its scheduler, and its compute adapters.
- **Acquisition**: the studio, where data enters a dataset.

Exploration is not a cornerstone: goofi-data relies on the existing dataset libraries on the
internet for it. The agent can search them, but their integration is not a cornerstone. Exchange
(remote repositories, shared datasets between people) is a feature of storage. Accounts are a layer
of the ecosystem, used by goofi-pipe and goofi-data alike, not a cornerstone of goofi-data.

The file has one section for the ecosystem ground goofi-data stands on, one per cornerstone, and
one for the interface. Each section holds its decisions, its remaining work and its open points.

## Ecosystem ground

### Decisions

- **One repository.** goofi-pipe, goofi-data and later goofi-build live in the goofi repository,
  one Cargo workspace, one lock file, one version. The repository is the unit of atomic change; a
  version boundary belongs only where code leaves our control. Independence is a property of the
  dependency graph, not of the repository. The earlier decision of a repository of its own
  (`../goofi-data`) is withdrawn.
- **The present program is renamed goofi-pipe.** "goofi" names only what is shared: the
  repository, the home `~/.goofi`, the account, the shared crates and the shared frontend library.
- **Three layers.** Shared base (timing, layout, session, payload types and `Meta`, transport,
  recorder) → goofi-pipe (graph, runtime, engines, bridge, CLI) → goofi-data. goofi-data builds on
  the base and never on goofi-pipe; neither application depends on the other. A situation in the
  tests crate asserts the rule from the crate graph.
- **Prefixes.** `goofi-` for shared ecosystem crates (the base crates keep their names), `gp-` for
  goofi-pipe crates, `gd-` for goofi-data crates. The present `goofi-build` crate (the node library
  builder) becomes `gp-build`, so the name is free for the model builder.
- **Layout.** All Rust crates in one flat folder; the prefix is the one owner of the layer. One
  shared UI library and one folder per frontend:

  ```
  crates/      goofi-*  gp-*  gd-*
  web/ui/      the shared library
  web/pipe/    the goofi-pipe frontend
  web/data/    the goofi-data frontend
  ```

  Each application gets a tests crate of its own, so a goofi-data CI job does not build the engines.
- **One goofi UI library.** One npm workspace package with subpath exports. It holds only what
  does not know an application: primitives, tokens, the panel mechanics from panelty, the plotting
  from plotluck, data viewers, the browser end of the transport and the op client. The patch editor
  and node inspector stay in the goofi-pipe frontend.
- **plotluck and panelty merge into the repository.** plotluck has no external users and merges
  back. panelty has one external user: it stays frozen on npm and GitHub at its current version,
  and a copy merges into goofi where development continues. The names panelty and plotluck do not
  persist; both become fully integrated into the local ecosystem.
- **Shared components, no dependency on goofi-pipe.** A shared component is a standalone unit
  with a standardized interface. The recorder and the data transport are the first two; more will
  be identified.
- **The recorder is shared.** goofi-data incorporates the recorder crate to record data inside
  goofi-data directly.
- **The data transport is shared.** goofi-data incorporates the data transport crate as its direct
  means to communicate with goofi-pipe when it runs a recipe that goofi-pipe implements.
- **Shared locations.** goofi-data integrates with goofi-pipe by sharing the home
  (`~/.goofi/recordings`, `config.toml`, the node roots). A goofi-pipe recording (its folder,
  manifest and sidecars) is one of goofi-data's native dataset formats. No second copy of the data.
- **Accounts.** One goofi account serves the whole ecosystem, for goofi-pipe and goofi-data alike.
  goofi-pipe will have a node library hosted on the internet, in the spirit of VCV Rack and
  ComfyUI: users share nodes and features and integrate them seamlessly into the system. goofi-data
  needs the same account and login infrastructure for shared datasets between users and for
  interfacing with remote storage systems such as Drive and Cloudflare. The account also serves
  the remote data repository and sharing between people.

### Where the work starts: the recorder and the transport

Before the goofi-data software, the transport and recorder layers are refined and standardized in
their robustness and their API. The first step is in the goofi repository, not in goofi-data. The
recorder needs this work also outside the scope of goofi-data.

- **The recorder's purpose is high-robustness data recording.** It does not meet that purpose
  now: it is regularly seen to drop frames under moderate load, and its dependency on ffmpeg for
  video is a nuisance.
- **The recorder is a standalone Rust crate**, semi-independent of goofi-pipe, in the goofi
  repository. It is a module a program imports and runs in a thread, or runs directly in a process
  of its own. The interface is the same in both forms.
- **Each program holds its own recorder.** goofi-pipe owns its recorder in process, as a thread,
  and keeps its present structure: no frame is serialised on the way to it. goofi-pipe and
  goofi-data running at the same time each hold their own recorder, and so does each of several
  goofi-pipe instances.
- **The recorder takes both forms of transport:** the in-process link from the threads of its
  program, and transport packets from another process. A Python subprocess node, for example,
  reaches the recorder through the transport.
- **The recorder's communication interface** does three things: it configures the recording
  location (a root folder), it configures the session (every recording goes into the session
  folder, inside the root folder), and it registers data streams.
- **A registered stream is a named, queued transport receiver** that the recorder holds. A sender,
  for example a goofi-pipe runtime, sends its data to that receiver. In goofi-pipe, to register a
  node in the recorder panel establishes this queued transport link between goofi-pipe and the
  recorder thread or process.
- **The recorder's queue drops no frame.** It is large enough for everything that comes in, and
  ideally has no pre-allocated size. It sits in the receiver's end: a producer only pushes, with
  no bookkeeping, and the receiver queues every frame and the recorder works through its queues.
- **File organization.** Tags, annotations and a metadata store are configured for a recording, so
  what the recorder writes is already a dataset goofi-data reads natively.
- **The data transport is a standalone Rust crate** in the goofi repository: a general real-time
  data transport protocol with a standardized interface.
- **One layer, one API.** The transport is not a complicated system. It is unified into a single
  standard layer with modes and configurations: one standardized API through which two points
  exchange data. The API is the same in process and across processes, queued and unqueued.
- **The mode is detected, not chosen.** The public code path is the same in both cases. The
  implementation detects the process of the source and of the target. In the same process it
  hands over the `Arc` with no serialisation. In different processes it serialises and sends
  through shared memory.
- **Modalities and channels.** The transport carries various modalities (array, string, table,
  ...) over various channels (in process, shared memory, WebSocket, ...).
- **Every packet holds a meta dictionary** with string keys. It allows what goofi-pipe's meta
  allows now (`goofi_core::Meta`).
- **Python bindings.** The crate exports Python bindings. A Python subprocess node uses the
  transport directly: it sends its packets peer to peer to other subprocess nodes, to the
  recorder, and to the reducer in the main goofi-pipe process.
- **No request pattern.** A node works by producing and triggering downstream, not by request.
  A subprocess node sends its data back to the main process itself, so the transport has no call
  and reply and no wait pattern. This overrules the earlier decisions of `data-layer.md` (no
  serialisation layer, no cross-process publish/subscribe); that entry is removed.
- **A subprocess node is independent.** It schedules and triggers itself, autotrigger included.
  The main process likely keeps no thread per subprocess node. What remains there is management:
  the processes stay alive and are restarted when needed.
- **Three uses, one transport:** between nodes in goofi-pipe; from the goofi-pipe backend to the
  frontend, after the reducer stage; and from goofi-pipe to the recorder.

### Built in the goofi-pipe layer

Work in the goofi repository, needed by goofi-data and built with it.

- **The one transport crate.** `goofi_transport` has slots, queues, doors and the exchange (a
  shared-memory mailbox each way and a loopback doorbell, called by request and reply) now, and
  depends on `goofi_supervisor` (timing, session, sync), `goofi_core` and `goofi_node`. It becomes
  the one API: a connection between two points, queued or unqueued, its mode detected from their
  processes, with the packet, the channels and the Python bindings. The exchange and the `rpc`
  call frames of `goofi_codec` go. The browser data plane (the GOOF frame, behind the reducer)
  becomes one of its channels. Its dependencies on goofi-pipe crates are cut or moved, so a
  program that is not goofi-pipe can build on it.
- **Independent subprocess nodes.** The Python subprocess tier and the hosted Rust tier
  (`goofi host`) move from called children to independent nodes on the transport.
- **The standalone recorder crate.** `goofi_record` depends on the same three crates and on the
  transport now. It keeps the transport and loses the rest: the communication interface (root,
  session, streams), the thread and process forms, no frame dropped, no ffmpeg, and the file
  organization (tags, annotations, metadata store). The recorder panel and the manager's armed set
  move onto stream registration.
- **The deterministic runtime mode.** goofi-pipe has playback nodes (`ArrayPlayback`,
  `StringPlayback`, `TablePlayback`, `goofi_core::replay`) and a harness-driven clock
  (`Clock::External`), but no mode that feeds a recording through a patch one frame at a time,
  with the patch clock driven by the recording's timeline, and records every output. It is a
  goofi-pipe op; goofi-data invokes it and does not run patches itself.
- **Linking a running patch.** The studio links up to a running goofi-pipe patch and launches a
  saved recording patch; both go through goofi-pipe's ops.
- **Account management.** The goofi account, its credentials in the home, and the ops that log in
  and out.
- **The online node library.** Upload a node bundle under the account, browse and download one
  into goofi-pipe. `library.md` owns the local library and Git sources; this is the hosted side.
- **The goofi UI library.** panelty, plotluck and the goofi-pipe UI components merged into one
  frontend library the whole ecosystem builds on.

### Remaining

- The rename to goofi-pipe and the prefixes, in one commit, when no other session has open work,
  before the transport work.
- The repository layout: the flat crates folder, the npm workspace, the per-frontend folders, the
  tests crate per application, the situation that asserts the layer rule.
- plotluck merged back; panelty copied in and frozen upstream.
- The data transport as a standalone crate: the packet and its meta dictionary, the modalities,
  the channels (shared memory, WebSocket), queued and unqueued connections, the detection of the
  mode, the Python bindings, and its dependencies on goofi-pipe crates removed. Then its three
  uses: node to node, backend to frontend, goofi-pipe to recorder.
- The subprocess tiers as independent nodes: what of the node thread moves into the child, the
  control connection, and the management of the processes. Run the children, python, nodes and
  recording situations on Windows and macOS after it; the shared-memory section and the wait
  compile on Windows but have not run there.
- The recorder as a standalone crate on the transport: the communication interface (root folder,
  session, stream registration), the thread form and the process form, the cause of the dropped
  frames found and fixed, video without ffmpeg, and the tags, annotations and metadata store.
- Account management and the online node library.
- The goofi-data binary, serving its own app the way goofi-pipe does.

### Open

- **How the shared crates leave their goofi-pipe dependencies.** Timing, session ownership and
  the frame types are in `goofi_supervisor` and `goofi_core`. Either the shared crates own what
  they need, or a smaller base crate holds it for both.
- **How goofi-data finds the home.** `goofi_supervisor::layout` owns every path. The path rule
  must be shared without a dependency on goofi-pipe.
- **Where `Meta` and the payload types live.** The transport and its Python bindings need them
  without a dependency on goofi-pipe; they are in `goofi_core` now.
- **The Python package `goofi`.** Whether it stays the node API of goofi-pipe or splits into the
  shared transport bindings and the node API.
- **Two binaries** (`goofi-pipe`, `goofi-data`) are assumed; one binary would link the engines
  into goofi-data.
- **The licence** of panelty and plotluck against AGPL-3.0-only is not checked.
- **The limit of a queue with no fixed size.** Today the queue is a ring of fixed depth
  (`record_depth`: 64 frames for signal, 1024 for audio, 4 for graphics) that drops its oldest.
  A graphics readback is megabytes; when the disk is slower than the producer for long, a queue
  with no bound takes all memory. Decide what the recorder does then, and what it reports.
- **What replaces ffmpeg** for video.
- **What of the node thread moves into the child.** Validated 2026-11-11: today a subprocess
  node has no management thread. Its node thread in the main process (`goofi_runtime::Runtime`,
  one per node of every tier) owns the child: it spawns it at the first call, sees its death in
  the wait of a call, and spawns and re-seeds a fresh one at the next call. The child ends itself
  when the parent dies (`child::watch_parent`). The same thread also does everything that is not
  the data call: it parks on the input bells and the pace, evaluates the expression bindings,
  applies each settle (slots, bells, bindings, armed outputs), stamps the frames, clears inputs
  and reports status. Besides `Process` the child is called for `Setup`, `ParamChanged`,
  `Refresh`, `Pulse` and `Stop`. Both children can link Rust (the hosted child is the goofi
  binary, the Python child loads the `goofi-pymod` wheel), so the runtime loop can run in the
  child. Decide that, and where an expression that reads another node (`nd('x')`) is evaluated.
- **The control connection.** A settle, a param edit, a pulse, an options refresh and a stop go to
  the child, and status and errors come back. Without a request pattern these are messages on a
  queued connection of the same transport; a refresh's options and a recording flush's
  acknowledgement are answers today.
- **Who sees a child die.** Today the death shows at the next call. With no calls, something
  must watch the process exit. A thread per node is not needed for it; decide the one place
  (the session already lists every child).
- **A queued connection across processes with no fixed size**, in shared memory.
- **The recorder's communication interface: its channel.** Whether the configuration (root,
  session, register) goes over the transport itself or over a separate control path when the
  recorder runs in a process of its own.
- **The word for a standalone program of the ecosystem**, as against a plugin.
- **The account service and its credential model.** The hosting service is not chosen.
  `plugins.md` carries the same question for its session/corpus plugin, the live-patch side of the
  same data.

## Cornerstone: storage

goofi-data organizes datasets and takes care of dataset file management, across modular storage
adapters and several locations, and exchanges datasets between places and people.

### The convention

- goofi-data defines its own file system convention, with a name (not chosen yet) and a validator
  that checks a dataset's adherence. The convention defines how metadata is stored for data and
  how data is organized hierarchically with shared groups across the hierarchy. It must support
  ANY data format, and generalize from complex and large datasets to small custom datasets without
  deep structure.
- **The hierarchy.** A generic hierarchy: the folder names decide the hierarchy level names. A root
  holds, for example, subject folders, and each subject folder holds `eeg` and `audio` folders.
  A group is defined by its name and its level in the hierarchy by default: the `eeg` folders of
  every subject at that level are one group, and goofi-data understands them as the same. Groups
  across levels of the hierarchy can be linked up easily, but goofi-data never links them
  automatically.
- **Properties.** A dataset carries the properties a model is built from: the person it was
  recorded from, tags for modalities and other labels, and its lineage. A property is set once on
  the dataset and read from there; nothing mirrors it.
- **History.** Each dataset contains its full history: the entire tree of recipes, each with its
  full declaration. Relatives are saved in a tree, and changes are tracked.

### The data layout

The layout is straightforward and deeply file based.

- **A sample** is an atomic segment of data that serves the downstream purpose: a single EEG
  epoch, a single image, a video segment, an audio segment; but also a full EEG recording, a set
  of images, or a full video file.
- **A sample is always a file, but the file may be a pointer.** A sample is either an individual
  file, or a chunk in a bigger file with a clear deterministic extent: the start and end time of
  an EEG epoch in a bigger file, an image crop, and so on. A chunk sample is a file that points to
  its source and defines the extent and the method of extraction (crop, epoch, cut, ...).
- **A file is identified by its path relative to the dataset root.** When a station supplies a
  chunk of a bigger file, the file's id (its relative path) lets one file hold multiple samples.
- **Grouping of sample files.** To reduce the number of files, the sample files within a folder
  are automatically grouped into a single file that points to the respective source files via
  relative paths.
- **Sources.** The source files may live in their own source folder, unless the full file itself
  is also a sample. A dataset can hold multiple kinds of samples; the same dataset can hold a full
  audio file as a sample and, at the same time, chunks of that same file as samples. Then the
  audio file lives in its own regular sample directory, the chunk samples live in their respective
  directories, and the chunk samples point to the source file.
- **Reference counting.** goofi-data automatically recognizes when a source file is being pointed
  to. When a recipe deletes a source file that is referenced, the file moves from its sample
  location (its regular relative path in the dataset) into the dataset's special sources
  directory: it is not deleted from the dataset, it no longer serves as a sample, and it still
  serves as a source for the chunk samples.
- **Epochs** are one derivative feature among many: derived samples in a group of the original
  file. They are not the focus of inspection.

### Archives, storage modes and adapters

- **Archives.** Packaging a dataset into one goofi-data archive for transfer is a central feature.
  Archiving compresses a dataset heavily and places it in the goofi archive format, to conserve
  space and to transfer between machines.
- **Two storage modes.** Archival storage holds the compressed package. Streaming storage holds a
  dataset in a way that supports direct access to its data. Not every storage interface supports
  both: Google Drive takes an archival adapter but likely no streaming one.
- **Storage adapters.** One dataset convention and model, implemented across modular storage
  interfaces: the local file system, Cloudflare, AWS, Hugging Face, Google Drive, SSH access to a
  remote machine's datasets, and more. Every adapter, the local file system included, follows the
  shared data model and implements one shared abstract interface.
- **Apache OpenDAL** is the backend layer under the storage adapters: one Rust API over S3 (and so
  Cloudflare R2), GCS, Azure, WebDAV, SFTP, Google Drive and more, each behind a Cargo feature.
- **Several locations.** goofi-data keeps track of several data locations at the same time.
- **Other formats.** goofi-data works with convention datasets and with other dataset formats: it
  tries its best to parse them, loads any dataset format, and converts common other frameworks'
  datasets to goofi-data. The abstract interfaces between the convention and other formats are
  standardized, so conversion and transfer go through efficient channels: moving a dataset
  between locations (AWS to local, local to Drive, ...). **BIDS is an import and export utility**,
  not at the core of goofi-data.

### Exchange

Remote repositories and shared datasets between people are native to data storage.

- goofi-data and goofi-build talk to one remote data repository that hosts personal data in the
  goofi-data format, shares datasets between people and holds collective datasets. The service
  is not chosen. Upload, download and sharing go through the goofi account.
- When a person sets up a dataset or an experiment, the ecosystem suggests dataset types and tags
  from the community's collective knowledge, so the recorded data's properties align with the
  properties of data in the ecosystem and collective research across aligned data is possible.
- The suggestion must not constrain: the person expands the ecosystem's properties as freely as
  they adopt existing ones. Suggesting only what exists is the failure to design against.

### Management and inspection

- **The dashboard.** Manage, view, inspect, archive and move datasets across adapters and between
  formats.
- **Inspection.** Standard metrics and inspections, and arbitrary Python commands run over the
  data: count certain files after a filter, list targeted information, and the like.

### Remaining

- Research (2026-11-07, done; the findings live in the planning page and its two reports, not
  here): storage platforms, external sources, and the structure of the convention.
- Name the convention and define it: metadata per level, groups by name and level, explicit
  links between groups across levels, the validator, the archive format, the streaming layout.
- The data layout: the sample file, the pointer file (source, extent, method), the grouped file
  per folder, the sources directory, reference counting, and the history inside the dataset.
- The shared abstract storage interface on OpenDAL and its first adapters; which adapters support
  archival, which streaming.
- Parsers and converters for other formats and standards (BIDS first, as a utility).
- The dashboard panels: overview across datasets, one dataset, a set of datasets compared,
  inspect history. Revise the Derive mock-up accordingly: epochs are one recipe among many, not
  the focus.
- The inspection surface: standard metrics, and Python commands over the data.
- The remote repository, upload, download and sharing; the suggestion mechanism and its balance
  between alignment and expansion.

### Open

- **The name of the convention** and of the archive format.
- **The pointer file's extent and method vocabulary** (crop, epoch, cut, ...): which methods are
  built in and how a station declares a new one.
- How suggestions weigh the community's existing properties against a new one, and what signal
  from the ecosystem feeds them.

## Cornerstone: recipes

A derivative is created by applying a transformation to a dataset. A derivative stores its source
and its transformation beside its data; the dataset's history holds every recipe with its full
declaration.

### The recipe

- **A derivative pipeline is a recipe.** A recipe is a graph: nodes are operations, edges are
  queues. It is shown node-based, with queue lines between the nodes, in the spirit of the Cooking
  for Engineers recipe layout (ingredients on the left, each step a column that spans the rows it
  combines) but more visually intuitive.
- **Stations and conveyor belts.** A station implements an algorithm that defines a given input
  and output format. A station may extract a sample into its own unit, or supply a chunk out of a
  file. The word for a consumer of a queue is *station*, as in a kitchen's stations, each doing
  one step, workers assigned to the station that needs them. "Transform" names what the station
  does, not the station.
- **Everything is transforms and queues.** This is the most general form and every step of the
  derivative process follows it. The initial queue holds the files the user selected, possibly as
  tuples (pairs, triples, ...) where one transformation needs several files, for example a merge
  of two files into one. Its consumer is data loading, which feeds a queue of loaded data, again
  possibly in tuples of paired samples. That queue is consumed by a transformation, followed by a
  queue, and so on, all the way to writer transformations that write the data back into files.
- **Cardinality modes.** One-to-one: one sample produces a new transformed sample (bandpass
  filtering). One-to-many: one sample produces multiple downstream samples (splitting a long
  recording into windows). Many-to-one: multiple samples merge into a single one (correlation of
  two samples).
- **Acyclic, with repetition of blocks.** The graph must be acyclic. The interface supports
  repetition of blocks: n repetitions of the same subgraph on the same set of inputs, producing n
  random derivative samples (one-to-many; bootstrapping for statistical analysis), or the same
  subgraph repeated n times taking its output back in as input (one-to-one repeated n times).
- **A goofi-pipe patch is one transformation.** A goofi-pipe patch run deterministically (every
  frame of the source through the patch, none dropped) is one transformation. It is a goofi-pipe
  op; goofi-data invokes it over the transport and does not run patches itself.
- **A derivation can be several steps**, for example filter, then notch, then re-reference. An
  intermediate is never saved to disk: it is kept in memory only as long as it is needed.
- **The configurator.** A component where procedures are selected intuitively: moving files and
  regrouping, a transformation applied to one file, to a set of files, to all files of a certain
  kind.
- **The run view.** The frontend of a running recipe is a dependency graph of steps, like a flow
  chart, with the progress represented across the edges.

### The scheduler

- **Left to right.** The scheduler works from source nodes to sink nodes.
- **One worker pool, shared across every queue.** Each consumer does one step of the
  transformation. For a dataset of 10k images this is a process pool applying the transformations,
  with parallel data-loading workers feeding a data queue; data workers are spawned automatically
  when a queue is consumed faster than it is produced, up to a configured maximum.
- **Emergently intelligent allocation.** The pool is split across the stations on a continuous
  trade-off between memory and compute: it maximizes compute utilization of the hardware while
  limiting the memory footprint. By default memory is limited to the RAM free on the machine,
  configurable to a lower target; consumers fill the CPU cores by default and can be limited. The
  scheduler assigns workers to queues dynamically and automatically from the structure of the
  derivative graph (the transform graph is the core; no deadlocks) and from the constraints,
  whether set by the user (RAM, compute) or by the hardware (free RAM, number of cores). The
  algorithm allocates workers to the best queue given the needs and the constraints.
- **Resource limits change on the fly.** The scheduler limits memory and CPU, and probably GPU.
  A limit can be changed while it runs: it constrains itself down or up and reconfigures its
  scheduling on the fly.
- **Fully resumable.** Recipes and the scheduler are fully resumable. Data is written regularly,
  so pause and resume, or a job's expiry and restart, go through seamlessly without losing
  progress.
- **Self-similarity.** Storage and recipes have the same shape, and so do the workers and queues
  inside a recipe.

### Compute adapters

- **Two dimensions, separated cleanly:** local versus remote, and direct versus Slurm. A recipe
  runs locally through the custom scheduler, or through Slurm on local or remote compute.
- **Slurm is a wrapper around the scheduler.** goofi-data's scheduler runs inside the Slurm job.
  Because the scheduler is resumable, a job that expires is restarted and continues.

### Remaining

- The recipe configurator: selecting procedures over files, sets and kinds; multi-step recipes;
  the node-based recipe view with queue lines; repetition blocks.
- The transform-and-queue runtime: the shared worker pool, tuple queues, loader and writer
  transforms, the cardinality modes, and the scheduler with its memory, CPU and GPU constraints,
  changed on the fly.
- The inner workings of the scheduler that drives optimized recipe execution.
- Resumability: the regular write, pause and resume, expiry and restart.
- The compute adapters: local direct, local Slurm, remote direct, remote Slurm.
- The run view: the flow chart with progress across edges.
- The deterministic runtime mode in goofi-pipe; derivatives as a tree with tracked changes.
- The dashboard panel for the recipe configurator and the run view.

### Open

- **The scheduler's inner workings**: the cost inputs per station, the allocation algorithm, and
  how the regular write for resumability relates to "an intermediate is never saved to disk".
- **How a goofi-pipe-implemented recipe runs** over the transport: which program starts goofi-pipe,
  and what crosses the transport.
- **GPU** as a scheduler resource: whether and how.

## Cornerstone: acquisition

Acquisition is where data enters a dataset: the studio.

### Decisions

- Recording in regular sessions must be very easy, through a preset configuration. Example: data
  recorded daily while writing and listening to music. goofi-data provides a UI to configure a
  recording studio that is opened each day and is linked to a dataset where the data accumulates.
- A preset is a **studio routine**: the studio holds them.
- Recording interfaces are modular. Common interfaces are supported directly: writing, audio
  recording, drawing, and the like. New custom recording interfaces come through a standardized
  abstract interface.
- One such path is goofi-pipe patches, which the recorder links up in two ways: an existing
  running goofi-pipe patch that goofi-data links up to, or a recording patch for goofi-pipe saved
  along goofi-data that the recording studio launches directly.
- The studio records through the shared recorder crate, so what it writes is already a dataset
  goofi-data reads natively (tags, annotations, metadata store).

### Remaining

- The recording studio: studio routines, the dataset a routine accumulates into, the built-in
  interfaces (writing, audio, drawing), the abstract recording interface, the goofi-pipe patch
  path.
- The dashboard panel for the studio.

## The interface

Not a cornerstone: the one way every cornerstone is operated.

- **One capability interface.** goofi-data follows the same CLI and op pattern as goofi-pipe.
  Everything that happens inside the UI goes through an op, which is also accessible directly via
  the CLI. Panels, CLI, MCP, scripts and the agent drive the same ops.
- **A native built-in agent** operates the software. It can search the existing dataset libraries
  on the internet.
- **Highly modular.** The software is composed of modules behind standardized interfaces: storage
  adapters, compute adapters, recording interfaces, stations.
- **The frontend** uses goofi's frontend stack (SvelteKit and the shared UI library) for offline
  data management. It does not use the panel layout engine in the same way as goofi-pipe.

### Open

- How the goofi-data frontend is organized, if not as a panel workspace.
