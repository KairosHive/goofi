# goofi-data

goofi-data is a platform of its own for datasets: it keeps, labels, derives, records and shares
them. It is an offline tool and a management system: a full data platform. Status: planning;
nothing is built. This file records the product decisions of 2026-11-06, 2026-11-07, 2026-11-11,
2026-11-12, 2026-11-13, 2026-11-14, 2026-11-15 and 2026-11-16. `goofi-build.md` holds the model
builder.

goofi-pipe records; goofi-data keeps, labels, derives and shares what was recorded; goofi-build
makes models from it. With the online node library, the three form one ecosystem: a meeting point
and a marketplace for autopoietic research and arts.

goofi-data has three cornerstones. A cornerstone is a core function that constitutes the software
and is composed with the other cornerstones:

- **Storage**: datasets, their layout, their adapters, and exchange between places and people.
- **Recipes**: derivation as a graph of transforms and queues, its scheduler, and its compute
  adapters.
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
  recorder) → goofi-pipe (graph, runtime, engines, bridge, CLI) → goofi-data. goofi-data builds
  on the base and never on goofi-pipe; neither application depends on the other. A situation in the
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
  (`~/.goofi/recordings`, `config.toml`, the node roots of `library.md`). goofi-data reads a
  goofi-pipe recording (its folder, manifest and sidecars) in place through a format adapter: no
  second copy of the data.
- **goofi-pipe and goofi-data are libraries too.** goofi-pipe is restructured so that it is used
  natively as a library; the same principle holds for goofi-data. goofi-data invokes goofi-pipe as
  a library and steps through a patch; it may not need the goofi-pipe ops for that.
- **One node graph implementation.** goofi-data reuses the implementation of the goofi-pipe node
  graph. The transport supports queued and unqueued modes: goofi-pipe runs unqueued, goofi-data
  runs queued. The lossless mode of goofi-pipe (no packet ever dropped, every pipe between nodes a
  queue) is a Kahn network: a goofi-pipe graph whose nodes are transforms and whose pipes are
  queues, scheduled by one node evoking the next in parallel as packets arrive, in data time, with
  the audio and graphics engines as nodes of the same network so their effects apply over datasets
  deterministically. The rules are `node-triggering.md`. Much of goofi-pipe is generalized to fit
  goofi-pipe and goofi-data both; this is a substantial change to the goofi-pipe codebase.
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
  now: it is regularly seen to drop frames under moderate load (no fixture reproduces this yet;
  `performance-audit.md` lists the measurement), and its dependency on ffmpeg for video is a
  nuisance.
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
  recording situations on Windows and macOS after it (`windows-cleanup.md` records the children,
  python and transport situations passing on Windows 11 on 2026-10-11 with `--test-threads=1`; the
  Windows CI job runs no situation).
- The recorder as a standalone crate on the transport: the communication interface (root folder,
  session, stream registration), the thread form and the process form, the cause of the dropped
  frames found and fixed, video without ffmpeg, and the tags, annotations and metadata store.
- **The base crates hold what the shared crates need.** `goofi_supervisor` (timing, layout,
  session, sync, worker, child) is base and stays. The payload types and `Meta` move out of
  `goofi_core` into a base crate that does not pull `goofi_view`. The transport's `names.rs`
  derives every name from the graph (`Uid`, `EventId`, `GraphView`, `Ringer`, `BoundVar`, `Var`
  of `goofi_node`) and leaves the transport. The recorder's `layout::Tool::Ffmpeg` and
  `child::run` go with "no ffmpeg"; its `replay::read_npy` moves with the payload types. Twelve
  crates depend on transport or record today.
- Account management and the online node library.
- The goofi-data binary, serving its own app the way goofi-pipe does.

### Open

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
  binary, the Python child loads the `goofi` wheel built from `goofi-pymod`), so the runtime loop
  can run in the child. Decide that, and where an expression that reads another node (`nd('x')`)
  is evaluated.
- **The control connection.** A settle, a param edit, a pulse, an options refresh and a stop go to
  the child, and status and errors come back. Without a request pattern these are messages on a
  queued connection of the same transport; a refresh's options is an answer over the exchange
  today, and a recording flush's acknowledgement is an in-process channel.
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
adapters and several locations, and exchanges datasets between places and people. It aims to be
the standard for dataset handling across data management, data loading, machine and deep learning
and archiving: an efficient native data store with a CLI-native manipulation system, which the
agent operates. Everything operates through goofi-data; the dataset is not human readable, and the
ops and the export utilities do the work.

### The dataset file

- **A dataset is one file.** It is memory mapped, append-only, and holds regions: a header; the
  blob region; the sample store; per embedding space a vector table and an index; a footer with
  the region offsets, a checksum and the blob index (per blob: hash, offset, length, codec, and
  for an array blob its dtype and natural shape). A blob has no inline header, so its offset
  points at the bytes. Moving a dataset is moving one file. One format serves local work and
  transfer. The sample layout is flat.
- **Compression is an op, not a mode.** Data is not compressed by default. `dataset compress` and
  `dataset decompress` are CLI ops, and the compressed file may carry a different suffix.
- **Two things, named differently: blob and sample.** A *blob* is a file's bytes: raw,
  page-aligned in the blob region, immutable once written, identified by the hash of its content,
  carrying a codec tag (none, or the codec of a compressed blob such as a video). A blob is never
  a sample. A *sample* is a view on a blob: `(blob, method, extent)` with its metadata inline. A
  whole file as a sample is the view with the full extent; an EEG epoch is `(blob, epoch, start
  to end)`; an image crop is `(blob, crop, x y w h)`. One blob serves many samples, and a file is
  a sample too, through the same rule.
- **A sample** is an atomic segment of data that serves the downstream purpose: a single EEG
  epoch, a single image, a video segment, an audio segment; but also a full EEG recording, a set
  of images, or a full video file. A dataset holds multiple kinds of samples; the same dataset
  holds a full audio recording as a sample and its chunks as samples, all views on one blob.
- **A sample may view several blobs**, one per stream, each with its own dtype, shape and rate,
  under one time base. A studio recording of audio and EEG is one sample in the selected dataset:
  an audio blob, an EEG blob, and one row that views both, paired and synchronized. The row's blob
  column is a list of `(stream, blob, extent)`; a transform that epochs the EEG views the EEG blob
  alone.
- **Writers write whole.** A one-to-many transform whose outputs share a shape writes them as one
  blob `[n, ...]`, each sample a view on index i: all epochs of a recording are one array on disk,
  memory mapped as a whole, and one sample each.
- **One read per sample.** A sample is one row of the sample store: its id (a hash of blob,
  method and extent), its blob (resolved through the footer's blob index to an offset), dtype,
  shape, strides, extent, method, kind (eeg, image, audio, ...), and a `meta` dictionary with the
  whole metadata of the sample inline, with mandatory fields per kind (`sfreq` for EEG, the
  aspect ratio for an image, ...) validated against the kind's schema. No second lookup before the
  bytes. The dtype decides the decoder. A sliding window over an uncompressed blob is offset
  arithmetic on the mapping; nothing is materialised.
- **The sample store is an embedded Arrow IPC file.** Columnar: filter millions of rows by kind
  and extent without touching the blobs; readable by pyarrow, polars and DuckDB given the byte
  range. A blob with codec `none` is `.npy`-compatible bytes: with offset, dtype and shape from the
  store, `numpy.memmap` reads it with no goofi code.
- **Append-only, tombstones, compact.** Nothing moves or is rewritten in place. A new blob is
  appended, a new store batch is appended, a new footer is written last; the old footer stays valid
  until the new one is complete, so a crash leaves a readable file. An edit is a new row plus a
  tombstone of the old. Offsets never shift. `dataset compact` rewrites the file without the
  tombstones and without blobs that no sample views.
- **Reference counting is derived from the views.** A blob stays while any sample references it:
  to delete "the recording as a sample" deletes a view, and the blob stays for the epochs that view
  it. A blob that no view references is garbage at compaction.
- **Editing is native and costs no copy.** The unit of change is the sample row. To delete three
  samples from a large dataset writes three tombstones; to change a property writes new rows and
  tombstones; to replace a recording's data writes one new blob and the rows that view it. A
  derivative applies in place when its target is the dataset it reads: it appends its outputs,
  tombstones what it replaces, and records itself in the history like any other run. Until
  `compact` reclaims them, the tombstoned rows and blobs are still there, so an in-place run is
  reverted by one more append. Space is reclaimed by `compact` alone, on demand, before a transfer
  or sync, or when dead bytes pass a fraction of the file; it needs twice the file's size while it
  runs.
- **Hashes make content portable; offsets are file-local.** Two transforms that produce the same
  bytes produce the same blob once; the same epoch cut twice is one sample. A Slurm run writes one
  file per shard and the shards merge by concatenation with rebased offsets.
- **The hierarchy and the groups** live in the sample store. A sample carries its path, the
  levels separated by slashes (`sub-01/eeg/...`); the level names are the dataset's (a root of
  subjects, each with `eeg` and `audio`, for example). A group is defined by its name and its
  level by default: the `eeg` level of every subject is one group, and goofi-data understands them
  as the same. Groups across levels can be linked explicitly; goofi-data never links them
  automatically. The convention generalizes
  from complex and large datasets to small custom datasets without deep structure, and supports
  ANY data format.
- **Properties.** A dataset carries the properties a model is built from: the person it was
  recorded from, tags for modalities and other labels, and its lineage. A property is set once on
  the dataset and read from there; nothing mirrors it.
- **History.** Each dataset contains its full history: the entire tree of recipes, each with its
  full declaration. Relatives are saved in a tree, and changes are tracked.
- **Epochs** are one derivative feature among many: derived samples in a group of the original
  recording. They are not the focus of inspection.
- **The spill is a dataset.** The scheduler's queue spill is a temporary goofi-data dataset
  created on demand: intermediate and ephemeral data, the blob region alone, raw aligned appends,
  written and mapped by the same code as every dataset.
- **The validator** checks a dataset's adherence to the convention. The convention has a name
  (not chosen yet).

### Storage adapters and locations

- **Storage adapters.** One dataset convention and model, implemented across modular storage
  interfaces: the local file system, Cloudflare, AWS, Hugging Face, Google Drive, SSH access to a
  remote machine's datasets, and more. Every adapter, the local file system included, follows the
  shared data model and implements one shared abstract interface.
- **Apache OpenDAL** is the backend layer under the storage adapters: one Rust API over S3 (and so
  Cloudflare R2), GCS, Azure, WebDAV, SFTP, Google Drive and more, each behind a Cargo feature.
- **A remote dataset streams.** The single file is read by range: the footer first, then the
  store, then the blobs a reader needs; OpenDAL serves range reads on every backend.
- **The adapter interface supplies state in structured form.** Each remote storage module
  implements the abstract interface, which requires certain functions that supply the storage's
  state in structured form. Not every remote platform supports streaming or remote inspection, so
  the information on the data may be limited; goofi-data enables and disables features from what
  the adapter reports.
- **Several locations.** goofi-data keeps track of several data locations at the same time. A
  dataset may live in multiple locations, for example remotely and locally.
- **The data directory.** goofi-data has one default (convention) data directory where all
  datasets are kept; external datasets, at a path outside the convention directory, are allowed.
- **Datasets are standalone and independent.** One dataset never depends on another. A derivative
  dataset b from a source dataset a tells that it came from a by remembering the operations that
  produced b from a, without naming paths or referencing a directly. Both exist side by side; if
  a is deleted, b is unaffected and sound. A derivative holds a copy of only what it needs; blobs
  are never linked between datasets, and every dataset file works when every other is deleted.
- **Other formats.** goofi-data works with convention datasets and with other dataset formats: it
  tries its best to parse them, loads any dataset format, and converts common other frameworks'
  datasets to goofi-data; import into the format from various other formats, and export into
  various formats. The abstract interfaces between the convention and other formats are
  standardized, so conversion and transfer go through efficient channels: moving a dataset
  between locations (AWS to local, local to Drive, ...). **BIDS is an import and export utility**,
  not at the core of goofi-data. A goofi-pipe recording is read in place through its format
  adapter.

### Exchange

Remote repositories and shared datasets between people are native to data storage.

- **Offline first, synced when online.** goofi-data is primarily an offline tool, and supports
  remote data hosts and the online account. When goofi-data is offline and records to a remote
  data platform, the data is stored locally until it is back online, then synced automatically to
  the remote storage; the same holds for updates on the remote. Data that is only remote needs no
  sync: it is streamed or loaded on the fly when needed.
- **The storage panel shows the unsynchronized state**, like `git status` and `git fetch`: local
  data waiting to become remote, remote data waiting to become local, and updates to remote data
  that are not synced yet. One click triggers the sync. Local data is removed only when it is
  certain that it reached the remote correctly.
- goofi-data and goofi-build talk to one remote data repository that hosts personal data in the
  goofi-data format, shares datasets between people and holds collective datasets. The service
  is not chosen. Upload, download and sharing go through the goofi account.
- When a person sets up a dataset or an experiment, the ecosystem suggests dataset types and tags
  from the community's collective knowledge, so the recorded data's properties align with the
  properties of data in the ecosystem and collective research across aligned data is possible.
- The suggestion must not constrain: the person expands the ecosystem's properties as freely as
  they adopt existing ones. Suggesting only what exists is the failure to design against.

### Embeddings and search

- **Embedding spaces.** A dataset declares embedding spaces by name (`clip`, `dino`), each with
  its dimension, dtype, metric and the transform that produces it. A sample holds zero or more
  vectors, each in one space: an image dataset carries CLIP vectors for text search and DINO
  vectors at the same time. The recipe that embeds a dataset is an ordinary transform that writes
  into a space; goofi-data stores vectors and does not own models.
- **One vector table per space**, not a column of the sample row: two parallel arrays, `ids [N]`
  (the sample hashes) and `vectors [N, d]` in the space's dtype, contiguous and page-aligned, so
  the vectors are a plain matrix. Adding a space to ten million samples touches no sample row; a
  space is dropped by dropping one table; a sample load does not read its vectors.
- **The vectors are accessible directly**, as memory-mapped arrays: `space info <name>` gives the
  offset, dtype and shape of the matrix and of the code plane, and the Python side returns the
  memmap, so a UMAP fits on a dataset's embeddings with nothing of ours in between.
- **Contiguity is a rule about writers.** The transform that embeds a dataset writes the space's
  table whole, as one slab at the end of its run (the vectors accumulate in the spill until
  then). A later append of single samples adds a tail slab; the footer lists the slabs; `compact`
  merges them into one. Between compactions `space info` reports the slabs.
- **Native vector search.** Every embedding is rotated by a seeded fast Walsh–Hadamard transform
  (a sign flip per coordinate, nothing stored) and reduced to the sign of each coordinate: one bit
  per dimension, d/8 bytes per vector (64 B at d = 512, 192 B at d = 1536). A query runs three
  stages, each a filter for the next: a scan of the codes (XOR and popcount, threads over chunks)
  keeps the best 4R; those 4R are rescored with the query at 4 bits per dimension and the best R
  kept; those R are re-ranked exactly from the mapped vector table, and the top k returned.
- **Two tiers, one scan.** While a space's code plane is under about 64 MB the scan is flat.
  Above, the plane is partitioned into lists (256 per 200k vectors, scaled with N), every vector
  routed by the same scan to its nearest centroid code; a query scans the centroid codes, takes
  the nearest P lists, and scans those. The centroids are trained with e-means (Meta FAIR, 2026;
  MIT; reimplemented): an exponential moving average of mass and position per centroid, a cosine
  step schedule, batch and step derived from n, k and the epochs, split revival of unused
  centroids, and an exact mean at the end. It needs no initialization scheme and no settings.
- **The index is derived.** It lives in the file as the code plane in row order plus a list id per
  row, is loaded as it is, and is rebuildable from the vector table in seconds. Append encodes,
  routes and pushes one row. Delete is the sample's tombstone, which the scan skips. `compact`
  regroups the plane and retrains the centroids when N has doubled since training.
- **One knob.** `accuracy` ∈ {fast, default, high} sets the re-rank depth R ∈ {100, 200, 500}
  and the lists probed P ∈ {4, 8, 16} per 256. It is calibrated at build on held-out vectors
  against an exact scan, so it means a measured recall on this data.
- **Measured** (2026-11-15, a probe on synthetic clustered data, 200k vectors at d = 512, 100
  queries, one core of this sandbox; estimates beyond it): a flat scan takes 0.95 to 1.7 ms and a
  partitioned query 0.1 ms, against 95 to 102 ms for the exact f32 scan. Recall@10 at R = 200 is
  0.999 on low-intrinsic-dimension data and 0.92 on hard data (0.97 with the 4-bit rescoring, 0.999
  at R = 500); 8 probes of 256 lists reach the flat scan's recall at 3 % of the work. The
  per-vector correction factor of RaBitQ gave nothing and is dropped; a full asymmetric scan costs
  4.5× and is used on candidates only. e-means reached the loss of 25 Lloyd iterations in 46 s
  against 381 s, with no empty list where Lloyd left 4 to 5; partition quality did not change
  recall at 8 probes. Isotropic data defeats one-bit codes (0.70 at R = 500), as it defeats every
  method.
- **Sizes.** Codes only, d/8 per vector: 1M × 512 is 64 MB, 1M × 1536 is 192 MB, 10M × 1536 is
  1.9 GB; plus 2 B per row and the centroids when partitioned. The f32 vector table at 1M × 1536
  is 6.1 GB and exists in any case.
- **Lean implementation**, pure Rust, no C++ dependency: rotation, encoding, the scan, the
  rescoring, the re-rank, e-means, the lists, the index region, the ops; about 325 lines plus
  about 150 lines of situations, under 1k with room for a 2-bit plane and the calibration.
- **The query op.** `sample search --space clip -k 10`, with the query vector from a file or
  `--like <sample id>`. A text query against a CLIP space needs the text encoder: a transform run
  on demand, not a storage feature.

### Management and inspection

- **The dashboard.** Manage, view, inspect, compress and move datasets across adapters and between
  formats.
- **Inspection.** Standard metrics and inspections, and arbitrary Python commands run over the
  data: count certain samples after a filter, list targeted information, and the like.

### Remaining

- Research (2026-11-07, done; the findings live in the planning page and its two reports, not
  here): storage platforms, external sources, and the structure of the convention.
- Name the convention and define the file: the header, the regions, the footer and the blob index,
  the sample store schema, the per-kind metadata schemas with their mandatory fields, the method
  and extent vocabulary, the hierarchy fields, the history records, the validator.
- Append, tombstone and `compact`; the footer protocol under a crash; the concatenation merge of
  shard files.
- The spill dataset.
- `dataset compress` and `dataset decompress`, and the suffix.
- The storage interface on OpenDAL and its first adapters; range reads for a remote dataset; what
  each adapter reports.
- Parsers and converters for other formats (BIDS first, as a utility; goofi-pipe recordings in
  place).
- Embedding spaces and the vector search: the code plane, the scan, the rescoring, the re-rank,
  e-means and the lists, the tiers, the knob and its calibration; `sample search` and
  `index rebuild`; situations for recall against an exact scan on a fixture, append then search,
  a tombstoned sample skipped, and a rebuild after `compact`.
- Experiments before the index is final: recall on real EEG, audio and image embeddings; scan rate
  and thread scaling on Apple silicon and Windows; the flat-to-partitioned switch point from those
  numbers; whether spaces at d ≤ 512 need a 2-bit plane.
- The dashboard panels: overview across datasets, one dataset, a set of datasets compared,
  inspect history. Revise the Derive mock-up accordingly: epochs are one recipe among many, not
  the focus.
- The inspection surface: standard metrics, and Python commands over the data.
- The remote repository, upload, download and sharing; the suggestion mechanism and its balance
  between alignment and expansion.

### Open

- **The name of the convention**, and the file suffix, plain and compressed.
- **The hash function** (BLAKE3 is the candidate), and whether the store's blob column holds the
  hash, the offset, or both.
- **The chunk methods** (crop, epoch, cut, ...): which are built in and how a transform declares
  one.
- **Annotations**: their time base and extent, and how a span survives a windowing transform.
- **A search combined with a property filter**: post-filter with over-fetch is the opinionated
  answer and fails on a very selective filter; decide when the filter language exists.
- How suggestions weigh the community's existing properties against a new one, and what signal
  from the ecosystem feeds them.

## Cornerstone: recipes

A derivative is created by applying a transformation to a dataset. A derivative stores its source
and its transformation beside its data; the dataset's history holds every recipe with its full
declaration.

### The recipe

- **A derivative pipeline is a recipe.** A recipe is a graph: nodes are transforms, edges are
  queues. The words are *transform* and *queue*. It is shown node-based, with queue lines between
  the nodes, in the spirit of the Cooking for Engineers recipe layout (ingredients on the left,
  each step a column that spans the rows it combines) but more visually intuitive.
- **A transform** implements an algorithm that defines a given input and output format. A
  transform may write a new blob, or supply a chunk sample out of an existing blob (a view, no
  copy). Workers are assigned to the transform that needs them.
- **Everything is transforms and queues.** This is the most general form and every step of the
  derivative process follows it. The initial queue holds the samples the user selected, possibly
  as tuples (pairs, triples, ...) where one transformation needs several samples, for example a
  merge of two into one. Its consumer is data loading, which feeds a queue of loaded data, again
  possibly in tuples of paired samples. That queue is consumed by a transformation, followed by a
  queue, and so on, all the way to writer transformations that write the data back into a
  dataset's blobs and samples.
- **Cardinality modes.** One-to-one: one sample produces a new transformed sample (bandpass
  filtering). One-to-many: one sample produces multiple downstream samples (splitting a long
  recording into windows). Many-to-one: multiple samples merge into a single one (correlation of
  two samples).
- **Acyclic, with repetition of blocks.** The graph must be acyclic. The interface supports
  repetition of blocks: n repetitions of the same subgraph on the same set of inputs, producing n
  random derivative samples (one-to-many; bootstrapping for statistical analysis), or the same
  subgraph repeated n times taking its output back in as input (one-to-one repeated n times). A
  repetition that feeds its output back is unrolled before scheduling: explicit in the backend,
  visually collapsed in the frontend.
- **A goofi-pipe patch is one transformation.** A goofi-pipe patch run in the lossless mode
  (every frame of the source through the patch, none dropped, in data time; `node-triggering.md`)
  is one transformation. goofi-data invokes goofi-pipe as a library (see the ecosystem ground); it
  does not run patches itself.
- **A derivation can be several steps**, for example filter, then notch, then re-reference. An
  intermediate is kept in memory as long as it is needed, and not saved to disk as a sample.
- **Checkpoints where the user says.** A queue has a setting to checkpoint data to disk as it
  comes through. Checkpointing is not strict beyond that.
- **Offloading to disk when RAM runs out.** When memory is out, given the constraints, the
  scheduler writes the intermediate representations it holds in memory to disk. It tracks which
  data is actively needed for an imminent transform and which can be unloaded because it is
  finished or needed only at a later stage, and offloads where necessary to free RAM. The spill
  is a temporary dataset created on demand (storage).
- **A guard around every transform.** A transform that fails, by a crash or out of memory, fails
  safely.
- **The configurator.** A component where procedures are selected intuitively: moving samples
  and regrouping, a transformation applied to one sample, to a set of samples, to all samples of a
  certain kind.
- **The run view.** The frontend of a running recipe is a dependency graph of steps, like a flow
  chart, with the progress represented across the edges.

### The scheduler

- **Left to right.** The scheduler works from source nodes to sink nodes.
- **One worker pool, shared across every queue.** Each consumer does one step of the
  transformation. For a dataset of 10k images this is a process pool applying the transformations,
  with parallel data-loading workers feeding a data queue; data workers are spawned automatically
  when a queue is consumed faster than it is produced, up to a configured maximum.
- **Emergently intelligent allocation.** The pool is split across the transforms on a continuous
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
- **Fully resumable.** Recipes and the scheduler are fully resumable. Checkpoints and offloaded
  data on disk let pause and resume, or a job's expiry and restart, go through seamlessly without
  losing progress.
- **Self-similarity.** Storage and recipes have the same shape, and so do the workers and queues
  inside a recipe.

### Compute adapters

- **Two dimensions, separated cleanly:** local versus remote, and direct versus Slurm. A recipe
  runs locally through the custom scheduler, or through Slurm on local or remote compute.
- **Slurm is a wrapper around the scheduler.** goofi-data's scheduler runs inside the Slurm job.
  Because the scheduler is resumable, a job that expires is restarted and continues.

### Remaining

- The recipe configurator: selecting procedures over samples, sets and kinds; multi-step recipes;
  the node-based recipe view with queue lines; repetition blocks.
- The transform-and-queue runtime: the shared worker pool, tuple queues, loader and writer
  transforms, the cardinality modes, and the scheduler with its memory, CPU and GPU constraints,
  changed on the fly.
- The inner workings of the scheduler that drives optimized recipe execution.
- Resumability: queue checkpoints, offloading, the transform guard, pause and resume, expiry and
  restart.
- The unrolling of fed-back repetitions, collapsed in the frontend.
- The compute adapters: local direct, local Slurm, remote direct, remote Slurm.
- The run view: the flow chart with progress across edges.
- The lossless mode in goofi-pipe (`node-triggering.md`); derivatives as a tree with tracked
  changes.
- The dashboard panel for the recipe configurator and the run view.

### Open

- **The scheduler's inner workings**: the cost inputs per transform, the allocation algorithm,
  and the offloading policy.
- **How a goofi-pipe-implemented recipe runs** as a library inside goofi-data: what of goofi-pipe
  is linked, and how the patch's nodes map onto transforms and queues.
- **Whether every recipe is a goofi-pipe Kahn network**, a mode of the goofi-pipe engines, with
  the dataset loader and writer as nodes, a sample as a delimited stretch of a stream, and
  goofi-data's scheduler over network instances, replicas and queue depths; or whether goofi-data
  keeps a transform-and-queue runtime of its own beside it.
- **The default for input slot coupling** in the queued mode (`node-triggering.md`).
- **GPU** as a scheduler resource: whether and how.

## Cornerstone: acquisition

Acquisition is where data enters a dataset: the studio.

### Decisions

- Recording in regular sessions must be very easy, through a preset configuration. Example: data
  recorded daily while writing and listening to music. goofi-data provides a UI to configure a
  recording studio that is opened each day and is linked to a dataset where the data accumulates.
- A preset is a **studio routine**: the studio holds them.
- **The studio is a raw data recording studio**: it records multimodal data in sync, with
  accurate timing across the modalities. goofi-data records without a running goofi-pipe patch.
- **Modalities and recorder types are modular, and mixed and matched.** A recording modality
  (the input module) follows a general abstract interface and supplies time-stamped chunks; the
  recorder type (the file type) follows the same principle and puts the chunks into a file, with
  the buffering, flushing and compression logic it needs. The input module does not know the
  recorder type: the input modality can be audio and the recorder type mp3. Common modalities are
  supported directly: writing, audio recording, drawing, and the like. New custom recording
  interfaces come through the abstract interface.
- One such path is goofi-pipe patches, which the recorder links up in two ways: an existing
  running goofi-pipe patch that goofi-data links up to, or a recording patch for goofi-pipe saved
  along goofi-data that the recording studio launches directly.
- The studio records through goofi-data's own recorder (one recorder per program), so what it
  writes is already a dataset goofi-data reads natively (tags, annotations, metadata store): a
  recorder type writes the dataset file directly, one blob per stream and one sample for the
  full recording, paired and synchronized across its modalities, with no conversion.

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
  adapters, compute adapters, recording modalities and recorder types, transforms.
- **The frontend** uses goofi's frontend stack (SvelteKit and the shared UI library) for offline
  data management. It does not use the panel layout engine in the same way as goofi-pipe.

### Open

- How the goofi-data frontend is organized, if not as a panel workspace.
