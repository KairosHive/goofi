# Control data

One representation for every value a person or a machine controls: a node param, a patch
variable, a control panel widget and a state machine attribute. Variables become producers on
the data plane, with the standing and the transport an output slot has.

Status: specification. This records the product decisions agreed through 2026-10-04. No
implementation has started. `state-machine.md` builds on it.

## Decisions

### One declaration, one carrier

- `goofi_core::control::Spec` is the one declaration of a control value's shape:
  `Num { dims, min, max, int, options, color }`, `Bool`, `Str { options, refresh }`, `Pulse`
  and `Array { dtype, shape }`. The first four are the forms a param allows today; `Array` is
  the form a drawing and a lookup table need. `ParamSpec` (`goofi-node`), the probe's spec
  (`goofi_core::probe`), `ControlKind`'s born value and the machine attribute all become
  readings of `Spec`. A `Spec` carries its own coercion (`coerce(Data) -> Data`): `Param::with_values`
  and `VariableValue::coerced_like` fold into it.
- The one value carrier is `goofi_core::Data`, the frame an output slot emits. A number is a
  `[1]` array, a vector a `[n]` array, a bool a `[1]` array read through `gate`, a string a
  `STRING` frame, a pulse an empty `[0]` array whose arrival is the fire. `Param` becomes
  `{spec: Spec, value: Data}` and keeps its typed accessors (`as_f64`, `as_vec`, …) as views;
  `VariableValue` and `goofi_graph::doc::Scalar` are removed. Everywhere a value crosses a
  thread, a process or the wire, it is a `Data` frame and takes the frame's path.
- The document literal of a value stays what a `.gfi` holds today: an untagged number, list,
  string or bool, written and read through `Spec`. An array value is a native data file in
  the archive (the recorder's format, `goofi-record`) at `variables/<name>`; the document
  holds its spec and the file's name.

### Variables are producers

- A variable is an output slot of the patch's own producer, `variables`, a `base` the
  manager holds like a node's. `output_service(base, "<group>.<element>")` is its data
  service; `view_door_service` serves its viewers. The reducer, the recorder and `node
  snapshot` reach it by the same names a node's slot has, so a variable can be viewed,
  recorded and probed like any slot.
- A param reads a variable as a stream. `variables.<g>.<e>` in an expression resolves to
  `BoundVar::Stream`, and the `reference` mode accepts `variables.<g>.<e>[i]` beside
  `node.slot[i]`. `BoundVar::Value` remains for `nd('x').params` and `me.params` only.
  `variable_as_param` and the inline re-send of every variable on every settle go away.
- A value write publishes a frame and nothing else: `VariableStore::set`, `follow`, `publish`
  and the machine's writes take the store's own lock, coerce through the spec, emit on the
  service, and return. The graph lock, `settle`, `replica()` and the doc broadcast are not on
  the value path; they are on the config path (add, remove, rename, retype, widget, source,
  lock). The follower thread shrinks to a tap sink; the reducer's `pick` already runs on the
  producer's own thread and writes through it.
- Values leave the document. `variables.<name>` holds `{spec, control?, source?, lock?}`. A
  value edit stays a command with an inverse that holds the previous frame; undo re-publishes
  it. Save takes the store's latest frames; load publishes them before the first settle.
- The frontend reads variable values as it reads slot values: `/variables` is a live socket
  restating every non-array variable at `LIVE_PERIOD`, as `/params/<node>` does for a node,
  and feeds the variables panel, the control panel widgets, the inspector chips and the state
  machine panel. An array variable is drawn through the data worker (`bindViewer` on
  `variables`, `<name>`) like a viewer. A knob drag previews the value edit; previews are
  cheap now and need no coalescing beyond the one-per-frame the control socket does.
- A followed variable (`source`) copies the picked element or the whole frame, by its spec:
  an `Array` spec follows the frame itself, with no index.
- `GraphView.variables` keeps the latest frames for the graphics planner's default size.
- A node that only reads a variable no longer re-evaluates on an unrelated settle; it wakes on
  the variable's frame like on any stream. `triggers` keeps its meaning.

### The control panel

- A widget is a `Control { kind, x, y, w, h }` over a variable whose `Spec` carries the range,
  step and options the widget drew from its own fields before. `min`, `max`, `step`, `options`
  move from `Control` into the spec; `control add` and `control edit` write both.
- `ControlKind` gains `Vector` and `Color` (a `Num` of several dims, and a colour), and
  `Paint` draws an `Array` variable `[h, w, 4]` of f32 RGBA in 0..1. `control paint {ops}`
  rasterizes the strokes in the manager with `drawing::raster` into the variable's array and
  publishes it; the stroke script is the edit, the array is the value. The base64 byte-code
  string, `drawing::encode/decode/append/to_value/from_value`, and the node-side rasterizing
  of a string are removed. A pad's size is the control's `w × h` at a chosen resolution,
  `control edit {resolution}`.
- `fits` becomes a reading of the spec: a knob draws a one-dim `Num`, a toggle a `Bool`, a
  paint pad a `[h, w, 4]` `Array`.

### Tests

- Extend `editing.rs` and `running.rs`: a variable followed from a slot, read by a param as a
  stream, observed through `probe` on the consumer; the value path proven to leave no doc
  patch; an array variable painted by op, viewed by `node snapshot --raw`, saved and reloaded
  whole; undo of a value edit re-publishing the previous frame.
- Playwright: the control panel's knob and paint pad over the live socket and the data worker;
  the variables panel listing a vector and an array.

## Open

- Whether a vector of two to four dims is also served by `/variables` or only by the data
  worker. The threshold is a number, `LIVE_INLINE_MAX`, set when the first wide one is drawn.
- Whether a `Pulse` variable is useful. The spec allows it; no widget draws one yet.

## Not to be done

- A second transport for small values. A `[1]` frame over iceoryx2 is the one path.
- Keeping `VariableValue` beside `Param` for the typed scalar case.
- A variable with history. Latest-wins, like a slot.
