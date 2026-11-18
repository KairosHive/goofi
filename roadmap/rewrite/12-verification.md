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

See the end of this file (12.9).

## 12.5 Coverage checks of this spec against the present program

Run on 2026-11-18 against the working tree.

| Check | Method | Result |
| --- | --- | --- |
| Every node file is in appendix A | The stems of the `.rs`, `.py` and `.wgsl` files under `node-bundles/` (hidden files excluded) are looked up in the names of appendix A, ignoring case and underscores. | 215 files, 0 missing. |
| Every op is in appendix B | The phrases of the op declarations under the bridge's `ops` folder are looked up in the first column of appendix B. | 101 ops, 0 without a row. |
| Feature surveys | Seven surveys read the whole code (shell, editor, viewers and panels, bridge and ops, model and foundation, engines, nodes and tests). Two audits compared their reports with this spec. | Part 12.8. |

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
