# goofi-data

goofi-data is a platform of its own for datasets: it keeps, labels, derives, records and shares
them. It lives in its own repository (`../goofi-data`). It builds in the goofi ecosystem and does
not depend on goofi: it runs without goofi installed. It supports goofi deeply and shares
implementations and standards with it. Status: planning. This file records the product decisions
of 2026-11-06, 2026-11-07 and 2026-11-11; nothing is built. The ideas are collected here until
goofi-data has its repository. `goofi-build.md` holds the model builder.

goofi records; goofi-data keeps, labels, derives and shares what was recorded; goofi-build makes
models from it. With the online node library, the three form one ecosystem: a meeting point and a
marketplace for autopoietic research and arts. The word for a standalone program of the ecosystem
is not chosen yet.

## Decisions

### Shared ground

- **Shared components, no dependency on goofi.** goofi and goofi-data build on the same
  components, and neither program depends on the other. A shared component is a standalone unit
  with a standardized interface. `panelty` and `plotluck` are independent repositories in this
  spirit already. The recorder and the data transport are the next two; more will be identified.
- **The recorder is shared.** goofi-data incorporates the recorder crate to record data inside
  goofi-data directly.
- **The data transport is shared.** goofi-data incorporates the data transport crate as its direct
  means to communicate with goofi when it runs a recipe that goofi implements.
- goofi-data integrates with goofi by sharing its locations: the home (`~/.goofi/recordings`,
  `config.toml`, the node roots). A goofi recording (its folder, manifest and sidecars) is one of
  goofi-data's native dataset formats. No second copy of the data.
- goofi-data uses goofi's frontend stack (SvelteKit, `panelty`, `plotluck`) and its op vocabulary:
  one capability interface, so panels, CLI, MCP and scripts drive the same ops. goofi-data's
  frontend is panelty for offline data management.
- **One goofi UI library.** goofi-data reuses goofi's UI and UX components. Eventually every UI
  component of the goofi ecosystem moves into one goofi frontend library that holds the common
  elements: panelty, plotluck and the other goofi UI components merged into one goofi UI lib.
- One goofi account serves the whole ecosystem: the remote data repository, the online node
  library (upload a node to share it, download a node into goofi) and sharing between people.

### Where the work starts: the recorder and the transport

The first step is in the goofi repository, not in goofi-data. The recorder needs this work also
outside the scope of goofi-data.

- **The recorder's purpose is high-robustness data recording.** It does not meet that purpose
  now: it is regularly seen to drop frames under moderate load, and its dependency on ffmpeg for
  video is a nuisance.
- **The recorder is a standalone Rust crate**, semi-independent of goofi, in the goofi
  repository. It is a module a program imports and runs in a thread, or runs directly in a process
  of its own. The interface is the same in both forms.
- **Each program holds its own recorder.** goofi owns its recorder in process, as a thread, and
  keeps its present structure: no frame is serialised on the way to it. goofi and goofi-data
  running at the same time each hold their own recorder, and so does each of several goofi
  instances.
- **The recorder takes both forms of transport:** the in-process link from the threads of its
  program, and transport packets from another process. A Python subprocess node, for example,
  reaches the recorder through the transport.
- **The recorder's communication interface** does three things: it configures the recording
  location (a root folder), it configures the session (every recording goes into the session
  folder, inside the root folder), and it registers data streams.
- **A registered stream is a named, queued transport receiver** that the recorder holds. A sender,
  for example a goofi runtime, sends its data to that receiver. In goofi, to register a node in
  the recorder panel establishes this queued transport link between goofi and the recorder thread
  or process.
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
- **Every packet holds a meta dictionary** with string keys. It allows what goofi's meta allows
  now (`goofi_core::Meta`).
- **Python bindings.** The crate exports Python bindings. A Python subprocess node uses the
  transport directly: it sends its packets peer to peer to other subprocess nodes, to the
  recorder, and to the reducer in the main goofi process.
- **No request pattern.** A node works by producing and triggering downstream, not by request.
  A subprocess node sends its data back to the main process itself, so the transport has no call
  and reply and no wait pattern. This overrules the earlier decisions of `data-layer.md` (no
  serialisation layer, no cross-process publish/subscribe); that entry is removed.
- **A subprocess node is independent.** It schedules and triggers itself, autotrigger included.
  The main process likely keeps no thread per subprocess node. What remains there is management:
  the processes stay alive and are restarted when needed.
- **Three uses, one transport:** between nodes in goofi; from the goofi backend to the frontend,
  after goofi's reducer stage; and from goofi to the recorder.

### goofi-data: the dataset lab

- goofi-data organizes, processes and derives datasets, and takes care of dataset file
  management. It also inspects datasets, and it records.
- **The convention.** goofi-data defines its own file system convention, with a name (not chosen
  yet) and a validator that checks a dataset's adherence. The convention defines how metadata is
  stored for data and how data is organized hierarchically with shared groups across the hierarchy.
  It must support ANY data format, and generalize from complex and large datasets to small custom
  datasets without deep structure.
- **The hierarchy.** A generic hierarchy: the folder names decide the hierarchy level names. A root
  holds, for example, subject folders, and each subject folder holds `eeg` and `audio` folders.
  A group is defined by its name and its level in the hierarchy by default: the `eeg` folders of
  every subject at that level are one group, and goofi-data understands them as the same. Groups
  across levels of the hierarchy can be linked up easily, but goofi-data never links them
  automatically.
- **Archives.** Packaging a dataset into one goofi-data archive for transfer is a central feature.
  Archiving compresses a dataset heavily and places it in the goofi archive format, to conserve
  space and to transfer between machines.
- **Two storage modes.** Archival storage holds the compressed package. Streaming storage holds a
  dataset in a way that supports direct access to its data. Not every storage interface supports
  both: Google Drive takes an archival adapter but likely no streaming one.
- **Storage adapters.** One dataset convention and model, implemented across modular storage
  interfaces: the local file system, Cloudflare's database, AWS, Hugging Face, Google Drive, SSH
  access to a remote machine's datasets, and more. Every adapter, the local file system included,
  follows the shared data model and implements one shared abstract interface.
- **Several locations.** goofi-data keeps track of several data locations at the same time.
- **Other formats.** goofi-data works with convention datasets and with other dataset formats: it
  tries its best to parse them, supports other standards (for example BIDS), loads any dataset
  format, and converts common other frameworks' datasets to goofi-data. The abstract interfaces
  between the convention and other formats are standardized, so conversion and transfer go through
  efficient channels: moving a dataset between locations (AWS to local, local to Drive, ...).
- **The dashboard.** Manage, view, inspect, archive and move datasets across adapters and between
  formats.
- **Inspection.** Standard metrics and inspections, and arbitrary Python commands run over the
  data: count certain files after a filter, list targeted information, and the like.
- **Properties.** A dataset carries the properties a model is built from: the person it was
  recorded from, tags for modalities and other labels, and its lineage. A property is set once on
  the dataset and read from there; nothing mirrors it.

### Recording: the studio

- Recording in regular sessions must be very easy, through a preset configuration. Example: data
  recorded daily while writing and listening to music. goofi-data provides a UI to configure a
  recording studio that is opened each day and is linked to a dataset where the data accumulates.
- A preset is a **studio routine**: the studio holds them.
- Recording interfaces are modular. Common interfaces are supported directly: writing, audio
  recording, drawing, and the like. New custom recording interfaces come through a standardized
  abstract interface.
- One such path is goofi patches, which the recorder links up in two ways: an existing running
  goofi patch that goofi-data links up to, or a recording patch for goofi saved along goofi-data
  that the recording studio launches directly.

### Derivatives: recipes, transforms and queues

- A derivative is created by applying a transformation to a dataset. Relatives are saved in a
  tree, and changes are tracked. The earlier decision stands: a goofi patch run deterministically
  (every frame of the source through the patch, none dropped) is one transformation, and a
  derivative stores its source and its transformation beside its data.
- Epochs are one derivative feature among many: derived samples in a group of the original file.
  They are not the focus of inspection.
- **The configurator.** A component where procedures are selected intuitively: moving files and
  regrouping, a transformation applied to one file, to a set of files, to all files of a certain
  kind. A derivation can be several steps, for example filter, then notch, then re-reference. An
  intermediate is never saved to disk: it is kept in memory only as long as it is needed.
- **A derivative pipeline is a recipe.** A recipe is shown node-based, with queue lines between
  the nodes, in the spirit of the Cooking for Engineers recipe layout (ingredients on the left,
  each step a column that spans the rows it combines) but more visually intuitive.
- **Everything is transforms and queues.** This is the most general form and every step of the
  derivative process follows it. The initial queue holds the files the user selected, possibly as
  tuples (pairs, triples, ...) where one transformation needs several files, for example a merge
  of two files into one. Its consumer is data loading, which feeds a queue of loaded data, again
  possibly in tuples of paired samples. That queue is consumed by a transformation, followed by a
  queue, and so on, all the way to writer transformations that write the data back into files.
- **One worker pool, shared across every queue.** Each consumer does one step of the
  transformation. For a dataset of 10k images this is a process pool applying the transformations,
  with parallel data-loading workers feeding a data queue; data workers are spawned automatically
  when a queue is consumed faster than it is produced, up to a configured maximum.
- **The scheduler.** The pool is split across the consumers by an emergently intelligent scheduler
  on a continuous trade-off between memory and compute: it maximizes compute utilization of the
  hardware while limiting the memory footprint. By default memory is limited to the RAM free on the
  machine, configurable to a lower target; consumers fill the CPU cores by default and can be
  limited. The scheduler assigns workers to queues dynamically and automatically from the structure
  of the derivative graph (the transform graph is the core; no deadlocks) and from the constraints,
  whether set by the user (RAM, compute) or by the hardware (free RAM, number of cores). The
  algorithm allocates workers to the best queue given the needs and the constraints.

### The remote repository

- goofi-data and goofi-build talk to one remote data repository that hosts personal data in the
  goofi-data format, shares datasets between people and holds collective datasets. The service
  is not chosen.
- When a person sets up a dataset or an experiment, the ecosystem suggests dataset types and tags
  from the community's collective knowledge, so the recorded data's properties align with the
  properties of data in the ecosystem and collective research across aligned data is possible.
- The suggestion must not constrain: the person expands the ecosystem's properties as freely as
  they adopt existing ones. Suggesting only what exists is the failure to design against.

## Built alongside, in goofi

These are work in the goofi repository, needed by goofi-data and built with it.

- **The one transport crate.** `goofi_transport` has slots, queues, doors and the exchange (a
  shared-memory mailbox each way and a loopback doorbell, called by request and reply) now, and
  depends on `goofi_supervisor` (timing, session, sync), `goofi_core` and `goofi_node`. It becomes
  the one API: a connection between two points, queued or unqueued, its mode detected from their
  processes, with the packet, the channels and the Python bindings. The exchange and the `rpc`
  call frames of `goofi_codec` go. The browser data plane (the GOOF frame, behind the reducer)
  becomes one of its channels. Its dependencies on goofi crates are cut or moved, so a program
  that is not goofi can build on it.
- **Independent subprocess nodes.** The Python subprocess tier and the hosted Rust tier
  (`goofi host`) move from called children to independent nodes on the transport.
- **The standalone recorder crate.** `goofi_record` depends on the same three crates and on the
  transport now. It keeps the transport and loses the rest: the communication interface (root,
  session, streams), the thread and process forms, no frame dropped, no ffmpeg, and the file
  organization (tags, annotations, metadata store). goofi's recorder panel and the manager's
  armed set move onto stream registration.
- **The deterministic runtime mode.** goofi has playback nodes (`ArrayPlayback`,
  `StringPlayback`, `TablePlayback`, `goofi_core::replay`) and a harness-driven clock
  (`Clock::External`), but no mode that feeds a recording through a patch one frame at a time,
  with the patch clock driven by the recording's timeline, and records every output. It is a goofi
  op; goofi-data invokes it and does not run patches itself.
- **Linking a running patch.** The studio links up to a running goofi patch and launches a saved
  recording patch; both go through goofi's ops.
- **Account management.** The goofi account, its credentials in the home, and the ops that log in
  and out.
- **The online node library.** Upload a node bundle under the account, browse and download one
  into goofi. `library.md` owns the local library and Git sources; this is the hosted side.
- **The goofi UI library.** panelty, plotluck and goofi's UI components merged into one frontend
  library the whole ecosystem builds on.

## Remaining

- Research (2026-11-07, done; the findings live in the planning page and its two reports, not
  here): storage platforms, external sources, and the structure of the convention.
- Name the convention and define it: metadata per level, groups by name and level, explicit
  links between groups across levels, the validator, the archive format, the streaming layout.
- The data transport as a standalone crate: the packet and its meta dictionary, the modalities,
  the channels (shared memory, WebSocket), queued and unqueued connections, the detection of the
  mode, the Python bindings, and its dependencies on goofi crates removed. Then its three uses in
  goofi: node to node, backend to frontend, goofi to recorder.
- The subprocess tiers as independent nodes: what of the node thread moves into the child, the
  control connection, and the management of the processes. Run the children, python, nodes and
  recording situations on Windows and macOS after it; the shared-memory section and the wait
  compile on Windows but have not run there.
- The recorder as a standalone crate on the transport: the communication interface (root folder,
  session, stream registration), the thread form and the process form, the cause of the dropped
  frames found and fixed, video without ffmpeg, and the tags, annotations and metadata store.
- The shared abstract storage interface and its first adapters; which adapters support archival,
  which streaming.
- Parsers and converters for other formats and standards (BIDS first).
- The recording studio: studio routines, the dataset a routine accumulates into, the built-in
  interfaces (writing, audio, drawing), the abstract recording interface, the goofi-patch path.
- The recipe configurator: selecting procedures over files, sets and kinds; multi-step recipes;
  the node-based recipe view with queue lines.
- The transform-and-queue runtime: the shared worker pool, tuple queues, loader and writer
  transforms, and the scheduler with its memory and compute constraints.
- The dashboard panels: overview across datasets, one dataset, a set of datasets compared,
  the recipe configurator, inspect history, the studio. Revise the Derive mock-up accordingly:
  epochs are one recipe among many, not the focus.
- The inspection surface: standard metrics, and Python commands over the data.
- The deterministic runtime mode in goofi; derivatives as a tree with tracked changes.
- The remote repository, upload, download and sharing; the suggestion mechanism and its balance
  between alignment and expansion.
- Create `../goofi-data`, a binary serving its own app the way goofi does.

## Open

- **How the shared crates leave their goofi dependencies.** Timing, session ownership and the
  frame types are in `goofi_supervisor` and `goofi_core`. Either the shared crates own what they
  need, or a smaller base crate holds it for both.
- **How goofi-data finds the home.** `goofi_supervisor::layout` owns every path in goofi. The path
  rule must be shared without a dependency on goofi.
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
- **Where `Meta` and the payload types live.** The transport and its Python bindings need them
  without a dependency on goofi; they are in `goofi_core` now.
- **How a goofi-implemented recipe runs** over the transport: which program starts goofi, and
  what crosses the transport.
- **The word for a standalone program of the ecosystem**, as against a plugin.
- **The name of the convention** and of the archive format.
- **The single word for a consumer of a queue** (a recipe is "queues and ___"). Candidates:
  *station*, as in a kitchen's stations, each doing one step, workers assigned to the station that
  needs them; or *stage*, the plain pipeline word. "Transform" names what the station does, not
  the station.
- The remote service and its credential model. `plugins.md` carries the same question for its
  session/corpus plugin, the live-patch side of the same data.
- How suggestions weigh the community's existing properties against a new one, and what signal
  from the ecosystem feeds them.
