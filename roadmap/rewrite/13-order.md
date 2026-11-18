# 13. Order of work, proposals and open decisions

## 13.1 Order of work

The rewrite is built bottom up, but a thin vertical slice comes first: one signal node whose
output is seen in a browser, through every layer. Each stage ends at a checkpoint: a program that
runs, with green gates (part 12.3), and the parts of the spec it covers done by the definition of
part 00.5.

| Stage | Content | Checkpoint | Parts to read |
| --- | --- | --- | --- |
| 0 | The decisions marked "before stage 1" in 13.3. The repository layout (part 01.3). The tests crates and CI gates as empty shells. | An empty workspace that builds and passes its gates. | 01, 13.3 |
| 1 | The slice: time, paths, session, log; frames; the wire format; a `latest` connection in process; one node contract in Rust; the signal engine with one node (`LFO`); the op framework with `node add`; a server; the op client, the replica and one line viewer. | `LFO` runs and its line moves in the browser; a situation and a browser test prove it. | 02.1–02.3, 02.5–02.8, 03.2–03.3, 04.1–04.5, 05.1, 06.2, 07.1, 08.1, 08.8, 09.6 |
| 2 | Foundation in full: the transport with every delivery and channel; building of components; the recorder with the files type; value descriptions; shared codecs. | Situations of timing, transport, codec, children, ready, logging, recording. | 02 |
| 3 | The patch in full: graph, sub-patches, params and sources, expressions, variables, history, previews, settle, replication, patch files, recovery. The CLI and its styles. | Situations of editing, subpatches, session, contracts, cli. A patch is built from the CLI alone. | 03, 07.1–07.2, 07.5–07.6, B |
| 4 | The node library: Rust, Python and WGSL contracts; discovery; the builder; both Python tiers; hosted nodes on the transport; the bundles `signal` and `eeg`. | Situations of nodes, python, running, param modulation. The EEG examples run headless. | 04, 05.0–05.1, 06.1–06.2, A |
| 5 | The shared UI library and the goofi-pipe app: shell, workspace, node editor, inspector, viewers, console. | The browser suite on desktop, phone and tablet. | 09, 08.1–08.5, 08.8, 08.11 |
| 6 | The audio engine with its bundle, MIDI and VST3. The graphics engine with its bundle and windows. Crossings between the planes. | Situations of audio, graphics, engines. Every example runs. | 05, A |
| 7 | Variables panel, control panel, MIDI learn, state machines and events, the machine panel. | Situation of machine; the browser specs of machine, variables and MIDI learn. | 03.5–03.6, 08.6–08.7, 08.9 |
| 8 | Recorder panel, agent harness and panel, guides, MCP, plugins and the two shipped plugins, demo mode. | Situations of agent, mcp, plugins, demo. | 07.2–07.4, 08.10, 08.12–08.13, 08.1.4 |
| 9 | The library with sources and bundles; the remaining bundles of appendix A ported under the conventions of part 04.9. | Situation of shipped: every node and every example loads. goofi-pipe is at parity (part 12.8). | 04.8–04.9, 08.14, A, 12.8 |
| 10 | The Kahn mode over the same graph and engines; loader and writer nodes. | Situation of kahn. | 06.3–06.5 |
| 11 | goofi-data storage: the dataset file, adapters, exchange, embeddings and search; the dataset recorder type. | The storage situations of part 12.1. | 10.1–10.3, 02.9 |
| 12 | goofi-data recipes, scheduler and compute adapters; the studio; the dashboard. | The recipe and studio situations; the browser suite of goofi-data. | 10.4–10.7 |
| 13 | The account, the online library, the remote data repository. | — | 01.7, 04.8.5, 10.2 |
| 14 | goofi-build (part 11). | — | 11 |

Rules of the order:

- A stage does not start on an open decision that it needs.
- The present program stays the reference for behaviour until stage 9 reaches parity. A
  behaviour that this spec does not state is read from the present program and then written
  into the spec.
- Each stage ports the situations and browser specs of its surface from the present program
  first, as the definition of its behaviour.
- A stage is small enough for one handover. A session that takes a stage reads: the index, part
  00, the parts the stage names, and part 12.

## 13.2 Proposals of this spec

The spec keeps every feature of the present program. In the places below it changes a name or a
behaviour, or adds one, to remove a known inconsistency. Each is a proposal until it is
confirmed.

| # | Proposal | Present state | Part |
| --- | --- | --- | --- |
| 1 | The op vocabulary of goofi-pipe: `patch` apart from `session`; `node` and `nodes` as one noun; `control` folded into `variable`; rename as `edit --name`; `history` and `op batch`. | 101 ops with the inconsistencies of appendix B.3. | 07.6 | 02.1–02.3, 02.5–02.8, 03.2–03.3, 04.1–04.5, 05.1, 06.2, 07.1, 08.1, 08.8, 09.6 |
| 2 | The op vocabulary of goofi-data (the compacted set of 2026-11-16). | Not confirmed. | 10.7 | 02 |
| 3 | Common scheduling params `rate` and `unit`. | `max_frequency` and `frequency_mode`. | 04.3 | 03, 07.1–07.2, 07.5–07.6, B |
| 4 | Coupling per input slot replaces the trigger flag of a slot. | Decided in `node-triggering.md`; the default is open. | 06.1 | 04, 05.0–05.1, 06.1–06.2, A |
| 5 | One `nodes/` folder in a workspace; the file states its engine. | `nodes_signal/`, `nodes_audio/`, `nodes_graphics/`. | 04.6 | 09, 08.1–08.5, 08.8, 08.11 |
| 6 | The library under `~/.goofi/library/`, the private bundle as `_local`. | `~/.goofi/custom/`; the present roadmap says `~/.goofi/nodes/`. | 04.8 | 05, A |
| 7 | A small manifest per bundle (name, author, description, licence). | A bundle is a bare folder. | 04.1 | 03.5–03.6, 08.6–08.7, 08.9 |
| 8 | Naming conventions for every shipped node, with renamed slots, params and some types (`Lfo`, `Llm`). | Mixed conventions. | 04.9 | 07.2–07.4, 08.10, 08.12–08.13, 08.1.4 |
| 9 | An event kind for slots and frames. | Decided in `triggers.md` as triggers; not built. | 02.6, 03.6.4 | 04.8–04.9, 08.14, A, 12.8 |
| 10 | A `Feedback` node on the signal plane and one way to declare feedback. | Audio and graphics have one each, declared two ways. | 04.2 | 06.3–06.5 |
| 11 | Hosted nodes hot-reload in every engine, audio included. | Audio loads a new library in process; VST3 needs a restart. | 05.0 | 10.1–10.3, 02.9 |
| 12 | Reduction for viewers that keeps what is seen (minimum and maximum for lines, area average for images) in one place. | Strided subsampling on the server and a second fold in the browser. | 02.10 | 10.4–10.7 |
| 13 | Live param values and node errors on one path each. | Several paths. | 08.1.2 | 01.7, 04.8.5, 10.2 |
| 14 | Shell additions: a New control, a save-as shortcut, notification tones. | No New control; one danger-styled toast. | 08.2 | 11 |
| 15 | Panel moves work by finger. | Panel docking has no touch path. | 08.3.3 |
| 16 | Knob: a fine modifier, a reset and text entry. | None of the three. | 09.3 |
| 17 | Inspector: a visible hint for the row keys; a Slots section with the coupling. | Hidden keys; no slot settings. | 08.5 |
| 18 | The console prompt on the control connection; filters and prompt history kept as panel navigation state. | HTTP exec; state lost on close. | 08.11 |
| 19 | The agent launcher offers model and effort; the plane removes an exited instance. | The op takes them, the launcher does not; each client removes it. | 08.12 |
| 20 | A plugin frontend can subscribe to events; a plugin panel has its own icon; one Python package and transport for nodes and plugin backends. | Panels poll; one fixed icon; two packages. | 07.4 |
| 21 | A plugin latency report from the audio engine; an error instead of a silent drop for a param value that cannot be applied. | Neither exists. | 05.2 |
| 22 | The recorder never drops; a dataset recorder type; no ffmpeg. | A bounded queue that drops its oldest; files only; ffmpeg for video. | 02.9 |
| 23 | One transport API with detected mode, no request and reply, hosted nodes as independent nodes. | Slots, queues, doors and an exchange with call and reply. | 02.8 |
| 24 | A library view in the app; a patch load that offers to install missing nodes. | Decided in `library.md`; not built. | 04.8 |
| 25 | A viewpoint per client that also holds the camera and the side-pane sizes. | One global blob; pane sizes in the browser. | 08.3.4 |
| 26 | The dtype of an array frame. | Float32 only. Open. | 02.12 |

## 13.3 Open decisions

Marked **(1)** when the decision is needed before stage 1.

**Ecosystem (part 01.10):** the names of the binaries and commands **(1)**; the word for a
program of the ecosystem; the account service and its credential model; the licence of panelty
and plotluck against AGPL **(1)**; one Python package or two; where the new tree lives while the
present program runs **(1)**.

**Foundation (part 02.12):** the dtype of an array **(1)**; what replaces ffmpeg; the memory
limit of the recorder queue; the channel of the recorder interface as a process; the unbounded
queue across processes; the default recorder type of goofi-pipe.

**Patch (part 03.11):** an event at the block or at the sample in audio; an event with or
without a payload; exact step onsets in audio; the limit between a variable value in the
document and in the workspace.

**Nodes (part 04.10):** shared graphics policies; private repositories and dependency
preparation; review or signing of published bundles.

**Engines (part 05.7):** isolation of plugin processing; a self-clocked audio engine; drift
between devices; MIDI input extent; VST3 silence and bend proofs; late renders in the interface.

**Modes (part 06.6):** the default coupling **(1)**; the depth of a queue.

**Ops (part 07.7):** confirmation of the vocabulary **(1)**; `agent attach`; shared graph ops in
goofi-data.

**goofi-pipe (part 08.15, 09.8):** the home of the graph canvas and the inspector, now that a
recipe is a goofi graph **(1)**; a narrow depth for topomaps; bandwidth to a tablet; the
library view as a panel or a dialog; a light theme; a command palette.

**goofi-data (part 10.8):** the name of the convention and the suffixes; the hash function;
chunk methods; annotations; search with a filter; sample markers; GPU as a resource; scheduler
internals; the remote repository and suggestions; the frontend organization.

**goofi-build (part 11.1):** everything beyond its purpose.

## 13.4 Relation to the other roadmap items

- This spec is the definition of the product for the rewrite. Where it differs from another
  roadmap item, the spec holds for the rewrite.
- `goofi-data.md`, `node-triggering.md`, `goofi-build.md`, `library.md`, `triggers.md`,
  `state-machine.md` and `plugins.md` record how their decisions were reached; their decisions
  are carried into parts 02 to 11.
- `reactivity.md`, `performance-audit.md`, `stability-audit.md`, `audio-engine.md`, the two VST3
  items, the two Windows items, `data-plane-bandwidth.md` and `frontend-performance.md` describe
  work on the present program. Their rules and reference values are carried into parts 05 and
  12; their defects are not carried, because the rewrite does not inherit the code.
- When the rewrite starts, the items of the first group are removed in the commit that makes the
  spec the one owner of their content.
