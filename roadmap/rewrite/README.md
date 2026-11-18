# The goofi ecosystem: a specification for a rewrite from scratch

This item is the declarative specification of the whole goofi ecosystem: goofi-pipe, goofi-data,
goofi-build and the goofi layer they share. It is written for a full rewrite from a clean slate.
It says what the system is and does, panel by panel, feature by feature, module by module. It
does not say how the code is arranged.

Status: specification only. No code of the rewrite exists. Written 2026-11-18 from a survey of
the whole present program and from the decisions of the roadmap items. This item does not follow
the normal form of the roadmap (decisions and remaining work only): it is the handover document
for the implementation.

## What the spec keeps

- Every feature of the present program (the parity lists of part 12.8, appendix A for nodes,
  appendix B for ops).
- Every principle and method that the present program proved (part 00).
- Every decision made for goofi-data, the Kahn mode, the library, events and state machines.
- It drops the patterns that grew by iteration: duplicate mechanisms, special cases and
  inconsistent names. Part 13.2 lists each place where the spec changes a present name or
  behaviour; each is a proposal until it is confirmed.

## Parts

The parts are in five groups:

- **Ground:** 00, 01.
- **The goofi layer** (shared by every program): 02, 03, 04, 05, 07.1 to 07.5, 09.
- **goofi-pipe:** 06.2 (the live mode), 07.6 (its ops), 08.
- **goofi-data:** 06.3 (the Kahn mode), 10. **goofi-build:** 11.
- **Proof and plan:** 12, 13, and the appendices.

| Part | File | Content |
| --- | --- | --- |
| 00 | `00-principles.md` | Design principles, policy, method, tests, definition of done, conventions of the spec. |
| 01 | `01-ecosystem.md` | Programs, layers, repository, running, home and session, trust, account, licence. |
| 02 | `02-foundation.md` | Time, paths, session, building, log, payload and stamps, wire format, transport, recorder, browser data plane, pitch. |
| 03 | `03-patch.md` | The patch: nodes, slots, cables, sub-patches, params, expressions, variables, state machines, arrangement, history, replication, files. |
| 04 | `04-nodes.md` | The node contract, manifest, params, the three languages, discovery and build, the library, conventions. |
| 05 | `05-engines.md` | Signal, audio and graphics engines; devices, MIDI, VST3, windows. |
| 06 | `06-modes.md` | The live mode and the Kahn mode. |
| 07 | `07-ops.md` | The op framework, CLI, MCP, guides, agent harness, plugins, the op vocabulary. |
| 08 | `08-pipe-app.md` | goofi-pipe: server, shell, workspace. |
| 08 | `08-pipe-editor.md` | goofi-pipe: node editor, inspector, control panel, variables panel. |
| 08 | `08-pipe-panels.md` | goofi-pipe: viewers, state machine, recorder, console, agent, plugin panels, library view. |
| 09 | `09-ui-library.md` | The shared UI library: tokens, primitives, gestures, panel mechanics, plotting, viewers, client. |
| 10 | `10-data.md` | goofi-data: the dataset file, adapters, exchange, embeddings and search, recipes, scheduler, studio, dashboard, ops. |
| 11 | `11-build.md` | goofi-build. |
| 12 | `12-verification.md` | Situations, browser suite, gates, coverage checks, measurements, parity lists, the validation of the Kahn rules. |
| 13 | `13-order.md` | Order of work, proposals, open decisions, relation to the other roadmap items. |
| A | `A-nodes.md` | The node inventory. |
| B | `B-ops.md` | Present ops and their place in the rewrite. |

## How to read it

- A session that implements a stage reads this index, part 00, the parts the stage names in
  part 13.1, and part 12.
- A section number (`8.4.2`) is a stable reference. Part 08 is in three files with continuous
  section numbers.
- "Open" marks a point that is not decided. Part 13.3 lists every open decision and marks the
  ones needed before the first stage.
- A behaviour that the spec does not state is read from the present program, and then written
  into the spec.

## Glossary

| Word | Meaning |
| --- | --- |
| goofi layer | Everything the programs share and that does not know an application. |
| patch | The document a person edits and a program runs: graph, variables, machines, arrangement, workspace. |
| recipe | A patch that goofi-data runs over a dataset in the Kahn mode. Its nodes are called transforms and its cables queues. |
| node | A unit of a graph. A leaf runs; a sub-patch holds members; a port is a boundary. |
| type | What a node is an instance of: `engine:Name`, one source file. |
| bundle | A folder of node files. |
| slot | An input or output of a node, with a kind. |
| cable | A connection from an output to an input. The op noun is `link`. |
| frame | One immutable value with its metadata and stamps. |
| stamps | What the runtime writes on every frame: instants, index, rate, origin, sources. |
| plane | The data of one engine: signal, audio, graphics. |
| crossing | A node that takes the data of another plane. |
| coupling | What an input slot does with a frame: `each`, `zip` or `hold`. |
| event | Something that happened once, delivered once to each listener. |
| settle | The one point after a batch where all consequences are decided from the change. |
| op | One capability of a program, declared once, reached by every interface. |
| live mode | The runtime mode of goofi-pipe: wall time, latest wins. |
| Kahn mode | The runtime mode of goofi-data: data time, queues, lossless and deterministic. |
| variable | A named value of a patch, `group.element`. |
| playhead | A position in a state machine, published as a variable group. |
| take | One recording from start to stop. |
| dataset | One goofi-data file. |
| blob | The bytes of one file inside a dataset. |
| sample | A view on one or more blobs with its metadata. |
| transform, queue | A node and a cable of a recipe. |
| studio routine | A preset for a recurring recording session. |
| home, runtime, session | What the user keeps; what one version needs; what one running program owns. |
