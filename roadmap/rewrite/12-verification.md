# 12. Verification

This part says how the rewrite is proven: the test surfaces, the gates, the parity lists against
the present program, and the checks and measurements made for this spec.

## 12.1 Situations

Each program has one tests crate. A situation is a named module that owns one surface and tests
it through the ops and the files a program writes (part 00.4). The harness boots a real program
in process with external clocks, a home of its own, no audio device and no native window, and
polls against one wait.

Situations of the goofi layer and goofi-pipe (the present situations, kept as surfaces):

| Situation | Surface |
| --- | --- |
| timing | The patch clock, waits, paces. |
| transport | Deliveries, in process and across processes; shutdown. |
| codec | The wire format against the golden file. |
| children | Ownership and death of child processes. |
| ready | Components prepared apart and swapped whole. |
| logging | Groups, limits, capture. |
| contracts | The op registry, the generated frontend types, the palette, the layer rule. |
| editing | Two clients, refusals, previews, undo and redo. |
| subpatches | Group, expand, ports, nesting. |
| running | Events, timers, coupling, stamps, input clearing, errors. |
| param modulation | Sources, expressions, direct reads, functionals. |
| nodes | Node files of a workspace, bundles, selection, archives. |
| python | Both Python tiers, the probe, the GIL rule. |
| engines, audio, graphics | Each engine under its external clock; crossings; readback. |
| machine | State machines and events. |
| recording | Takes, files, sidecars, manifests, the dataset type, no loss. |
| session | Save, load, recovery, patch files. |
| inspect | The text an agent reads. |
| browser | The control and data connections, the replica, the origin guard. |
| demo, headless | What each mode withholds. |
| plugins | Ops, hooks, panels, nodes of a plugin. |
| agent, cli, mcp | The harness on a PTY; the CLI styles; the MCP tool. |
| shipped | Every shipped node loads; every example patch loads without a warning. |
| kahn | One patch over one recording twice at different speeds: equal outputs (part 06.5). |

Situations of goofi-data: dataset file (append, footer protocol under a crash, tombstone,
compact, shard merge, validator); adapters (range reads against a local stand-in); search
(recall against an exact scan on a fixture, append then search, a tombstone skipped, rebuild
after compact); recipes (cardinality, repetition, replication order, checkpoint, offload, the
guard, pause and resume); scheduler limits changed during a run; studio (a multimodal take is one
sample); ops.

## 12.2 The browser suite

- Playwright against the real binary, one backend per worker. Projects: desktop, phone, phone in
  landscape, tablet.
- Specs (the present specs, kept): socket and convergence; integrity (a structural sweep of every
  panel and every param face); inspector and show rules; viewer; machine by gesture; variables
  and control layout; recorder; console; agent on a real PTY; MIDI learn; metadata; save options;
  recovery; touch. A second configuration covers plugin panels.
- Component tests cover the pure logic of the frontend; components are type checked.

## 12.3 Gates

- One CI job per gate, each on a runner of its own, each under ten minutes: the Rust gate of
  each program (linter without warnings, situations); the browser gate; one platform gate per
  operating system that can answer differently (Linux, macOS, Windows), each running situations.
- The setup of a job: provision; one build that fails on a warning; one boot-only run; a check
  that a second build compiles nothing.

## 12.4 Validation of the Kahn rules

A discrete-event model of the Kahn rules (part 06.3) was run on 2026-11-18. It is a probe in
Python, not the implementation. A scheduler picks at random, at each step, one node among those
that can progress; queues are bounded with a random depth from 1 to 8; a sender on a full queue
waits. Nodes: finite sources with irregular and equal instants, timers, periodic engines with
`hold` inputs, an accumulator that depends on order, a filter, a node that emits 0 to 3 frames
per event, seeded noise, and a replicated stateless node. 60 random graphs of 10 to 25 nodes with
mixed couplings, 60 % of them with a feedback edge; 200 schedules per graph; 12 000 runs per rule
set.

| Rule set | Runs that end | Deadlocks | Graphs with more than one output |
| --- | --- | --- | --- |
| The first wording of the rules (`node-triggering.md`) | 324 | 11 676 | 16 of 60 |
| The rules of part 06.3 | 12 000 | 0 | 0 of 60 |

Each correction was removed alone from the full rule set, to show that it is needed:

| Removed correction | Deadlocks | Graphs with more than one output |
| --- | --- | --- |
| A total order of equal instants (ties by arrival instead) | 0 | 60 |
| Progress as a bound on the next frame (progress as the instant of the last run instead) | 9 495 | 15 |
| Progress published before the sends | 1 877 | 0 |
| A sender does not wait inside one instant | 1 959 | 0 |
| `hold` queues drain by progress | 7 584 | 0 |
| An ended node discards its inputs | 5 080 | 0 |
| A `zip` group ends when one slot ends | 10 200 | 0 |
| `zip` by instant (pairing by index instead) | 6 539 | 47 |
| Strict progress on a feedback edge ("previous instant" instead) | 4 000 | 6 |
| A `hold` read waits for progress | 373 | 60 |
| The reorder stage owns the progress of a replicated node | 187 | 18 |
| Timers end at the horizon of the run | all 160 runs of 8 graphs did not end | — |

Minimal graphs reproduce each failure and pass under the full rules at every depth from 1 to 8
(200 schedules each): a burst into a diamond; a dense `hold` input; two sources with equal
instants into two `each` slots (191 to 194 different outputs with ties by arrival, 1 with the
total order); a feedback on itself with two events at one instant; a loop with a second source;
a `zip` of two rates; an ended node with a `hold` input.

Limits of the result: the campaign is a report of a subagent, read from its output file; the
minimal graphs were run again in this session with the same counts. Seeds are fixed. A step of
the model is atomic, so the behaviour of real threads is not covered. The open points are in
part 06.6.

## 12.5 Coverage checks of this spec against the present program

Run on 2026-11-18 against the working tree.

| Check | Method | Result |
| --- | --- | --- |
| Every node file is in appendix A | The stems of the `.rs`, `.py` and `.wgsl` files under `node-bundles/` (hidden files excluded) are looked up in the names of appendix A, ignoring case and underscores. | 215 files, 0 missing. |
| Every op is in appendix B | The phrases of the op declarations under the bridge's `ops` folder are looked up in the first column of appendix B. | 101 ops, 0 without a row. |
| Feature surveys | Seven surveys read the whole code (shell, editor, viewers and panels, bridge and ops, model and foundation, engines, nodes and tests). Two audits compared their reports with this spec, and a third read the spec alone. | Part 12.9. |
| Cross-references | Every "part N.x" reference is looked up in the section headings of the spec. | 0 broken. |

The surveys and the audits are reports of subagents. They were not verified line by line.

## 12.6 Size of the present program

Measured 2026-11-18 by line counts of the working tree (all lines, comments included).

| Part | Lines |
| --- | --- |
| Backend crates without tests (Rust, Python, WGSL) | about 54 800 |
| The tests crate | 21 302 |
| Frontend (`.svelte` and `.ts`, tests included) | 38 642 |
| Node bundles | 36 148 |

Largest backend parts: the bridge 12 171; the graph 7 809; audio 7 722; core 4 221; graphics
3 247; signal 3 188; supervisor 2 719.

## 12.7 Performance references

Tests do not assert performance. These probes are run by hand and recorded with date and
machine. The values the present roadmap records are the references to meet again:

- A param edit at 500 nodes: 16 to 20 ms at the median (the present `reactivity.md`); the target
  of the rewrite is a settle whose cost follows the change, under 1 ms at 500 nodes.
- A hop between two signal nodes: under 5 µs busy.
- A producer capped at 1 kHz delivers 1 kHz. A viewer capped at 60 per second gets 60.
- The audio control tick, the graphics tap lag, late renders and contended callbacks on a real
  device.
- Recorder workers against a fast producer, an audio pair and 1080p60 video: no loss.
- Vector search: the values of part 10.3 on real embeddings.
- A 5 s browser trace of a full example patch: idle, with the inspector open, and during a
  param drag.

## 12.8 Parity lists

The rewrite is at parity when each list holds.

- **Panels (10):** empty, node editor, inspector, viewer, control, variables, state machine,
  recorder, console, agent; plus plugin panels.
- **Viewer kinds (5):** line, image, brain, string, table.
- **Slot kinds:** array, string, table, audio, texture; the rewrite adds event.
- **Param faces:** number, integer with options, vector, colour, envelope, boolean, string,
  string with options, pulse, list section, colormap.
- **Control widgets (10):** knob, slider, number, text, toggle, dropdown, paint, vector, colour,
  envelope.
- **Expression terms:** `nd`, `me`, `vars`, `sm`, `t`, `np`, math, `lfo`, `nse`, `qtz`, `gte`.
- **Engines and tiers:** signal (Rust in process, Rust hosted, Python in process, Python
  subprocess), audio (Rust, VST3), graphics (WGSL, Rust and Python texture sources).
- **Recorder files:** NPY, CSV, text, WAV, video, sidecar, manifest, annotations.
- **Flags:** port, bind, extra nodes, load, headless, mcp, demo, debug, boot only.
- **Interfaces:** app, CLI (human and compact styles, batch on standard input, completions),
  console prompt, MCP, plugins (backend ops and hooks, frontend panels and header entries,
  nodes).
- **Examples (5):** psd-topomap, powerbands-osc, musical-features, connectivity-spectrogram,
  harmonic-observatory. **Skills (3)** and **shipped plugins (2)** of part 07.
- **Nodes:** appendix A. **Ops:** appendix B.
- **Tags (16)** of part 04.2.
- **Multi-client:** one history, presence, previews.
- **Devices:** touch, tablet, desktop, both orientations.

## 12.9 Audits of this spec

Run on 2026-11-18, after the first full draft. Each audit is a report of a subagent; its findings
were read and applied, not verified line by line.

| Audit | Method | Result |
| --- | --- | --- |
| Frontend coverage | The three frontend surveys against parts 08 and 09. | 32 behaviours that the spec did not state; all are now in parts 07 to 09. The auditor estimates that about 390 of about 450 survey statements were covered before the additions. |
| Fresh-context review | A reader with the spec alone looked for contradictions, broken references, unclear terms, gaps and structure problems. | 16 contradictions corrected; the references repaired; one word per thing in the glossary; the dataset file placed in the goofi layer; decisions and parts named per stage in part 13.1. |
| Backend coverage | The four backend surveys against parts 01 to 07, 12 and the appendices. | See 12.10. |

Findings of the fresh-context review that stay open: the spec gives no control protocol, document
schema, file layout or shared-memory layout (they are implementation, read from the present
program before the stage that needs them; part 13.1); appendix A lists nodes without their slots
and params (the node files are the reference); the rename table for the conventions of part 04.9
does not exist yet.
