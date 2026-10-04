# State machines and control data

A panel that hosts machines. A machine is a set of states, the transitions between them, and
the playheads that travel through them. A playhead writes the values its states hold into
patch variables, and any param reads them the way it reads a control panel today.

The same work unifies control data: a param, a variable, a control panel widget and a machine
attribute hold one kind of value, and variables become producers on the data plane with the
standing and the transport an output slot has.

Status: specification. This records the product decisions agreed through 2026-10-04. No
implementation has started. Build on `AGENTS.md` and the code; this file carries only what the
code cannot say.

## Decisions

### Control data is an array or a string

- The one value carrier is `goofi_core::Data`, the frame an output slot emits, in two of its
  forms: `Array` and `Str`. A variable, a state value, a widget's value and a param's held
  value are nothing else. `VariableValue`, `goofi_graph::doc::Scalar` and the typed `Param`
  variants as value holders are removed. `Param` keeps a node's declaration (number, bool,
  string, pulse, with range, `int`, `options` and `color`) as the TARGET a reader converts to,
  not as a form the data takes.
- Conversion is one rule, on read, from the source form: a number is the first element of an
  array, or a string parsed; a vector is the array, filled or cut to the target's dims; an int
  is rounded; a bool is a non-empty string or an array with any element above zero; a colour
  is a `[4]` array; a string is the string, or an array printed; a pulse fires on the rising
  edge of the bool reading. `goofi_core::control::read(&Data, &Param) -> Param` is the one
  owner; `Param::with_values`, `VariableValue::coerced_like`, `mailbox::value_as` and the
  evaluator's result coercion fold into it.
- A document literal of a value is an untagged number, list, string or bool, read into an
  array or a string; a `.gfi` writes a small array back as a number or a list and a wide one
  as a native data file (the recorder's format) at `variables/<name>`.

### Variables are producers

- A variable is an output slot of the patch's own producer, `variables`, a base the manager
  holds like a node's. `output_service(base, "<group>.<element>")` is its data service and
  `view_door_service` its viewers' door. The reducer, the recorder, `node snapshot` and the
  frontend's `/data/variables/<name>` reach it by the same names a node's slot has. The
  reducer and the data worker are generalized where a base that is not a node needs it; one
  infrastructure serves node producers and the variables producer.
- A variable has no tick. Its service is opened with a history of one, so a subscriber that
  connects after the last write receives that frame on connection. Node slots keep no
  history. Every retrieval of a variable yields its value: a new wire, a new viewer, a new
  binding, a reload.
- A param reads a variable as a stream: `variables.<g>.<e>` in an expression resolves to
  `BoundVar::Stream`, and the `reference` mode accepts `variables.<g>.<e>[i]` beside
  `node.slot[i]`. `BoundVar::Value` remains for `nd('x').params` and `me.params` only.
  `variable_as_param` and the inline re-send of every variable on every settle go away. A
  node that reads only a variable wakes on its frame like on any stream; `triggers` keeps
  its meaning.
- A value write publishes a frame and nothing else: `VariableStore::set`, `follow`, `publish`
  and the machine's writes take the store's own lock, emit on the service, and return. The
  graph lock, `settle`, `replica()` and the doc broadcast are the config path (add, remove,
  rename, widget, source, lock). The follower thread shrinks to a tap sink.
- Values leave the document. `variables.<name>` holds `{control?, source?, lock?}`. A value
  edit stays a command whose inverse holds the previous frame; undo re-publishes it. Save
  takes the store's latest frames; load publishes them before the first settle.
- The frontend reads every variable value through the data worker (`bindViewer` on
  `variables`, `<name>`), as it reads a slot. No second socket. The variables panel, the
  control panel widgets, the inspector chips and the machine panel subscribe this way; a knob
  drag previews the value edit through the control socket's one-per-frame coalescing.
- A followed variable (`source`) copies the picked element, or the whole frame when no index
  is given.
- `GraphView.variables` keeps the latest frames for the graphics planner's default size.

### The control panel

- A widget is a `Control { kind, min?, max?, step?, options, x, y, w, h }` as today; range,
  step and options are presentation, the widget's own. `ControlKind` gains `Vector` and
  `Color`. `Paint` draws an `[h, w, 4]` f32 RGBA array in 0..1: `control paint {ops}`
  rasterizes the strokes in the manager with `drawing::raster` into the array and publishes
  it; the stroke script is the edit, the array is the value. The base64 byte-code string,
  `drawing::encode/decode/append/to_value/from_value`, and the node-side rasterizing of a
  string are removed. A pad's resolution is `control edit {resolution}`.
- `fits` becomes a reading of the frame: a knob, a slider and a number draw a `[1]` array, a
  toggle any frame, a text, a dropdown a string, a vector a `[n]`, a colour a `[4]`, a paint
  pad an `[h, w, 4]`.

### Attributes travel as variables

- A playhead is a variable group. Its name is its group name, held by the one rule a control
  panel's group is held by (`Graph::group_taken`). Each attribute of the machine is one variable
  `<playhead>.<attribute>` in that group. The machine is the writer: it publishes frames on
  the variables' services like a follower does, with no undo entry, no dirty mark and no
  graph lock, equality-gated, paced on the viewer cap. A param links to an attribute with
  `variables.<playhead>.<attribute>`, as an expression or a bare reference, through the drop
  and the "select for reference" gestures the control panel already has. There is no second
  link mechanism.
- Three elements of a playhead's group are the machine's own and are refused as attribute
  names: `state` (string: the state the playhead is in, or is moving to), `from` (string: the
  state it left, empty at rest) and `progress` (float 0..1 along the transition, 1 at rest).
  An expression reads them like any variable; the panel animates from them.
- The group is config-locked for the life of the playhead; the variables are value-locked
  against every writer but the machine. Removing the playhead removes the group. Renaming it
  rewrites every expression that reads it (`rename_group`).
- A `.gfi` carries the playhead's variables like any other. On load the machine resets every
  playhead to its start state and publishes them. A reload does not resume mid-transition.

### Machines

- The document gains the root `machines: {name: Machine}`. A machine name is an identifier.
- `Machine { attributes: {name: Attribute}, states: {name: State}, transitions: {id: Transition},
  playheads: {name: Playhead} }`. `Attribute { default: literal, control }`: the default
  frame and the widget a state card and a playhead row draw it with, exactly as a control
  panel draws a variable. State and playhead names are identifiers; a transition id is
  minted by the manager (`t1`, `t2`, …) and never typed by a person except to address one.
- `State { pos: [x, y], values: {attribute: literal} }`, each literal an array or a string in
  the document form. A state may leave an attribute out: a playhead entering it keeps the
  value it holds. A removed attribute is removed from every state.
- `Playhead { color, start: state }`. Playheads of one machine share the attribute set. Their
  colour is theirs alone and is what the canvas draws them as.
- A playhead is in exactly one state, or on exactly one transition. Several active states
  are several playheads. A machine with no playhead runs nothing.

### Transitions

- `Transition { from, to, triggers: [Trigger], duration, curve, weight }`. `from` is a state
  name or `*` (any state). `to` may equal `from`: a re-entry restarts the dwell.
- A transition fires when any of its triggers fires. Trigger kinds, covering the decision
  space:
  - `manual`: the `machine fire` op, or a tap on the transition in the panel.
  - `after { seconds, chance }`: when the playhead has dwelt `seconds` in `from`, roll
    `chance` (default 1). A failed roll re-arms the same dwell. `seconds` may be an expression,
    in the one namespace below, read on entry.
  - `when { expression }`: the one expression language params use, read over `variables.*`
    and `t` only. It fires on the rising edge of its gate (`gate(x)`). A node output enters as a
    variable that follows it (`variable entry source`), so a node drives a machine through the
    same seam a MIDI knob drives a widget.
  - `meet { policy }`: fires when a second playhead arrives in `from`. `policy` chooses who
    takes the transition: `fifo` (the longest resident), `lifo` (the newest arrival), `all`
    (every resident). `alone` is the inverse: fires for the remaining playhead when the
    second-to-last leaves.
- When several transitions out of one state fire in the same tick, one is drawn by `weight`
  (default 1; 0 is never drawn). This is also how a random branch is spelled: several
  `after` transitions of the same dwell with different weights.
- `duration` is seconds; 0 is instant. `curve` is one of `step`, `linear`, `in`, `out`,
  `in_out` (quadratic), `smooth` (smoothstep). `goofi_core::ease` owns the curves; nothing
  else in the tree eases yet.
- Interpolation starts from the values the playhead holds now, never from the state it left,
  so a redirection while in flight is continuous. Arrays of one shape interpolate
  elementwise; a string, or arrays of two shapes, switch on arrival. A `step` curve switches
  everything on arrival.
- While in flight, only `manual` can redirect a playhead. The target state's triggers arm on
  arrival; the dwell starts on arrival.

### Where it runs

- `goofi_graph::machine` owns the model and the stepping: `Machines::advance(now, variables)
  -> Vec<(variable, Data)>`, pure and clocked by the one patch `Time`. It compiles
  `when` and `after` expressions through the graph's `ExprEvaluator` with `Local::Value`
  locals, the way a `BoundVar::Value` binding is evaluated, so the test evaluator covers it.
- One manager thread, `goofi-machines`, drives it on the viewer-cap pace: it reads the
  variables' latest frames from the store, calls `advance`, and publishes the writes. It takes
  no graph lock; the graph lock is for config edits, which replace its model. Easing is
  sampled at the viewer cap; an audio-rate ramp is a `Slew` node's job, not the machine's.
  Random draws use a seeded generator the test can fix (`machine edit {seed}`).

### Ops

Under the `machine` phrase, all commands with inverses unless marked:

- `machine list`, `machine add {name}`, `machine remove`, `machine rename`, `machine edit {seed?}`.
- `machine attribute add {machine, name, value, control?}`, `edit`, `remove`, `rename`.
- `machine state add {machine, name, pos?, values?}`, `edit {pos?, values?}` (a `values` key
  set to null clears that attribute from the state), `remove`, `rename`.
- `machine transition add {machine, from, to, triggers?, duration?, curve?, weight?}`, `edit`,
  `remove`. A transition is addressed by its id.
- `machine playhead add {machine, name, color?, start}`, `edit`, `remove`, `rename`.
- Effects, not undoable and not dirtying: `machine fire {machine, playhead, transition}`,
  `machine jump {machine, playhead, state}` (instant, no easing), `machine reset {machine}`.
- Previews: a state drag previews `machine state edit {pos}` as a node drag does.

### Panel

- Panel type `machine`, state `{machine}`, as `control` holds `{group}`. An empty panel
  offers the machines and a "new machine" entry.
- A SvelteFlow canvas, as the node editor's, with the shared camera, snapping and touch
  gestures (`editor/camera.ts`, `snap.ts`, `doubleTapZoom.ts`). States are cards with a name
  and one widget row per attribute the state sets; an unset attribute is a dimmed row that a
  tap sets. Edges are the `straight` edge type; a transition's trigger summary is its label.
  A self-transition is a loop at the card's corner.
- Each playhead is a coloured dot. At rest it sits on its state's card; in flight it moves
  along the edge from `from` to `state` at `progress`, interpolated between replica updates on
  the paint loop so it is smooth at the cap. A card lists the playheads in it in arrival order.
- A side pane holds the machine's attribute list and playhead list. A playhead row shows each
  attribute's live value with a chip `variables.<playhead>.<attribute>` that drags onto any
  param field or control widget. The selected transition's pane edits triggers, duration,
  curve and weight.
- Tapping a transition fires it for the playhead(s) in `from`; a long-press opens its pane.

### Tests

- Extend `editing.rs` and `running.rs`: a variable followed from a slot, read by a param as a
  stream, observed through `probe` on the consumer; a binding made after the last write
  receives that frame; the value path proven to leave no doc patch; an array variable painted
  by op, viewed by `node snapshot --raw`, saved and reloaded whole; undo of a value edit
  re-publishing the previous frame.
- `goofi-tests/tests/all/machine.rs`: one session builds a machine through ops, binds a
  param through `variables.<playhead>.<attr>` with `FirstVar`, fires a transition and polls
  the param through `probe` on the consumer and the playhead variables through their
  services; an `after`
  transition is reached by polling, never by sleeping; an eased transition is sampled and the
  samples are monotonic and end at the target; `meet` with each policy; a `when` trigger from
  a followed variable; save, reload, start state. Undo walks the edits back.
- Playwright: the control panel's knob and paint pad over the data worker; the variables
  panel listing a vector and an array; a desktop and a phone session open the machine panel,
  add two states and a transition by gesture, fire it, and see the dot on the target card. A
  layout integrity run covers it.

## Open

- Whether a `Pulse` target reads an array variable's edge or its arrival. Edge is chosen,
  matching the reference gate.
- Whether a string switches on arrival or on departure. Arrival is chosen; a gate that must
  open at the start of a move is a second attribute with its own transition.
- Whether a `when` expression may reference a node output directly (`nd('x').out`). Not now:
  it would make the machine a stream consumer. A followed variable is the seam.
- Transition priority beyond `weight`: an explicit order is not offered until a patch asks.

## Not to be done

- A machine as a node. It would run on one engine's clock and tie its outputs to slots;
  the variable system already reaches every param on every plane.
- A second value type for attributes, or a second link path beside the variable.
- A second transport or socket for small values. A `[1]` frame over iceoryx2 is the one path.
- A variable with history beyond the one frame a late subscriber needs. Latest-wins, like a
  slot.
- Audio-rate easing in the machine.
- Hierarchical or nested machines. Several machines in one patch compose through variables.
