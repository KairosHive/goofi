# State machines and control data

A panel that hosts machines. A machine is a set of states, the transitions between them, and
the playheads that travel through them. A playhead writes the values its states hold into
patch variables, and any param reads them the way it reads a control panel today.

The machines themselves are built: `goofi_graph::machine` is the model and the stepping, the
`machine` ops build one, the `goofi-machines` thread drives it, and a playhead is an owned
variable group `<playhead>.{state, left, progress, <attribute>…}`. What remains is the panel.

Status: in progress. This records the product decisions agreed through 2026-10-04. Build on
`AGENTS.md` and the code; this file carries only what the code cannot say.

## Decisions

### Control data is an array or a string

- `Data` is the one value and `control::read` the one conversion. The array carrier is f32,
  so a literal holds seven significant digits and a whole number up to 16,777,216 exactly;
  every read of an f32 yields the f64 its shortest decimal denotes, so `0.97` reads as `0.97`.

### The control panel

- `ControlKind` gains `Vector` and `Color`: a vector draws a `[n]` array, a colour a `[4]`,
  through `fits` as the other kinds do. Unbuilt; it lands with the panel stage, whose
  Playwright session lists a vector.

### Panel

- Panel type `machine`, state `{machine}`, as `control` holds `{group}`. An empty panel
  offers the machines and a "new machine" entry.
- A SvelteFlow canvas, as the node editor's, with the shared camera, snapping and touch
  gestures (`editor/camera.ts`, `snap.ts`, `doubleTapZoom.ts`). States are cards with a name
  and one widget row per attribute the state sets; an unset attribute is a dimmed row that a
  tap sets. Edges are the `straight` edge type; a transition's trigger summary is its label.
  A self-transition is a loop at the card's corner.
- Each playhead is a coloured dot. At rest it sits on its state's card; in flight it moves
  along the edge from `variables.<playhead>.left` to `.state` at `.progress`, interpolated
  between replica updates on the paint loop so it is smooth at the cap. A card lists the
  playheads in it in arrival order.
- A side pane holds the machine's attribute list and playhead list. A playhead row shows each
  attribute's live value with a chip `variables.<playhead>.<attribute>` that drags onto any
  param field or control widget. The selected transition's pane edits triggers, duration,
  curve and weight.
- Tapping a transition fires it (`machine fire`) for the playhead(s) in its `from`; a
  long-press opens its pane. A card drag previews `machine state edit {pos}` as a node drag does.
- A machine's playhead groups are owned: `variable_groups.<playhead>.machine` names the machine,
  and the variables panel draws them locked whole, as it draws a device's group.

### Tests

- Playwright: the variables panel listing a vector and an array; a desktop and a phone
  session open the machine panel, add two states and a transition by gesture, fire it, and
  see the dot on the target card. A layout integrity run covers it.

## Open

- Whether a `Pulse` target reads an array variable's edge or its arrival. Edge is chosen,
  matching the gate a bare stream has today.
- Whether a string switches on arrival or on departure. Arrival is chosen; a gate that must
  open at the start of a move is a second attribute with its own transition.
- Whether a `when` expression may reference a node output directly (`nd('x').out`). Not now:
  it would make the machine a stream consumer. A followed variable is the seam.
- Transition priority beyond `weight`: an explicit order is not offered until a patch asks.
- Every reader of a variable subscribes to its wire, the default `variables.system.*`
  expressions of every node included: a service takes 256 subscribers, so a patch of more
  nodes than that reading one variable is refused by the transport. Raise the ceiling, or
  share one subscription per process, when a patch asks.
- An expression a machine cannot evaluate is logged under `machines` once per message. Whether
  the panel should show it beside the transition, as a variable's `error` rides the replica.

## Not to be done

- A machine as a node. It would run on one engine's clock and tie its outputs to slots;
  the variable system already reaches every param on every plane.
- A second value type for attributes, or a second link path beside the variable.
- A second transport or socket for small values. A `[1]` frame over iceoryx2 is the one path.
- A second expression language, or a Rust subset for the audio plane. Python on a worker, one
  block behind, is the one computed path; a bare target is the one fast path.
- Python inside a device callback, a node thread or the graphics ticker, ever.
- A variable with history beyond the one frame a late subscriber needs. Latest-wins, like a
  slot.
- Audio-rate easing in the machine.
- Hierarchical or nested machines. Several machines in one patch compose through variables.
- A playhead's variables in the `.gfi`. The machine re-derives them from its model on load,
  so the file carries the model alone, as it carries no device's group.

## Stages and continuation after context compaction

Build in this order. Each stage ends at a tested commit, with the full local checks green, and
is a point to compact the context. Read this file and `AGENTS.md` first; inspect the diff since
the last stage's commit; keep this file current by deleting what has shipped.

1. **The panel.** The canvas, the cards, the dot, the side pane, the Playwright sessions, and
   the `Vector` and `Color` control kinds.

**Handover, 2026-10-04.** The Machines stage shipped: `goofi_graph::machine::{Machine, Machines}`
(the document record and the stepping), `Command::{SetMachine, RenameMachine, RenamePlayhead,
RenameAttribute}`, the `machine` op phrase, `goofi_bridge::machines` (the `goofi-machines`
thread, driven by `Machines::advance` on the viewer cap, writing through `VariableStore::drive`),
and the `machine::` situation. The machine's own playhead element for the state left is `left`,
not `from`: `from` is a Python keyword and no variable element may be one. The panel stage is
next and has not started; `panels/ControlPanel.svelte` and the node editor's canvas are what it
builds on, and `graphDoc.ts` already reads `machines` as a root and an owned group's lock.

Roadmap maintenance does not start implementation. Wait for the user's build instruction.
