# 03. The patch

A patch is the document a person edits and a program runs. In goofi-pipe it is a live patch. In
goofi-data it is a recipe. The graph model, its rules, its history and its file format are the
goofi layer and are the same in both.

## 3.1 The parts of a patch

| Part | Content |
| --- | --- |
| Graph | Nodes, cables, sub-patches. |
| Param state | Per node: literals, sources, the zero point, viewer state, armed outputs, slot couplings. |
| Variables | Named values with their configuration: groups, controls, expressions, locks. |
| State machines | Machines with attributes, states, transitions and playheads. |
| Arrangement | The workspace layout: tabs, splits, panels and the state of each panel. |
| Viewpoint | The camera and scope of each editor. Saved with the file, not part of history. |
| Workspace | A folder: node sources of the patch, node state blobs, wide variable values, skills. |

Not part of a patch: running instances, health, evaluated values, viewer demand, the status of a
recording, the roster of agents, the position of a playhead. These are runtime state. They reach
a client as events or as variable frames.

## 3.2 Nodes and identity

- A node is one of three kinds: a **leaf** (it runs), a **sub-patch** (a scope with members), a
  **port** (a boundary of a sub-patch).
- Every node has: an identity, a name, a position, its scope (the sub-patch that holds it, or
  the root), the viewer state per output slot, its zero point, and its armed outputs with their
  quality.
- A leaf also has: its type, its literals per param, its sources per param or element, and the
  coupling per input slot where it differs from the default.
- **Identity** is a number that is never reused, written as 12 hex digits. An undo restores the
  identity of a deleted node. A load restores the identities of the file. A paste makes new
  ones.
- **Type** is written `engine:Name` (`signal:Psd`, `audio:Osc`, `graphics:Blur`). A bare name
  resolves when exactly one engine offers it. The patch stores the resolved reference of the
  type (name, engine, bundle and source; part 04.8.2), so two bundles can hold a type of the
  same name. The structural types are bare: the sub-patch, and the input
  and output port of each slot kind.
- **Names:** one namespace for all three kinds. A name is a letter followed by letters and
  digits, and is not a reserved word. A new node gets `<type stem in lower case><N>`. A rename
  refuses an illegal or taken name, and rewrites every expression that spells the old name: in
  param sources, in variables and in machines.
- An expression refers to a node by name. Everything else refers to a node by identity.
- A node whose type is unavailable stays in the patch with its last known manifest, shown as
  unavailable with the reason.
- **Restart** rebuilds a node in place: values that the type still declares stay, new defaults
  are added, and sources and cables on params and slots that no longer exist are removed.
- When a leaf is removed, each of its consumers is rewired to the first source of the removed
  leaf whose kind fits. Panels bound to a removed node are unbound.
- `node add` with an illegal or taken name gives the node a minted name.

## 3.3 Slots, cables, sub-patches

- A slot has a name and a kind. An input slot also has: its default coupling (part 06.1), a
  list flag (it takes several cables, in connection order) and a required flag.
- Slot kinds: **array**, **string**, **table** and **event** (the wire kinds, which any engine
  carries), and **audio** and **texture** (each local to its engine). An event slot carries
  events (3.7).
- A cable joins an output to an input of the same kind.
- A plane is crossed only by a node that declares the foreign kind on an input: an audio node
  with an array input, a graphics node with an array or audio input, a signal node with an audio
  or texture input. A node cannot declare an output of the kind of another engine.
- Both ends of a cable are in the same scope. A cable across a scope boundary goes through a
  port.
- An output fans out without limit. An input takes one cable; a second cable replaces the
  first. A list input takes many.
- To add a cable that exists changes nothing.
- A cycle is legal in the graph. A loop runs only when a node in it declares feedback (one frame,
  block or tick of delay); otherwise its members do not run and each shows the reason.
- **Sub-patches:** a port has one slot of a fixed kind. The slots of a sub-patch are its ports in
  authored order, labelled by the port names. To group nodes splits each crossing cable at a
  port; cables from one output share one port. To expand a sub-patch joins the halves again.
  Sub-patches nest. An engine never sees a port or a sub-patch: it gets the leaf-to-leaf edges.
- The error of a sub-patch is the first error among its members.
- A sub-patch is not a shared instance: every copy is independent.

## 3.4 Params, sources and expressions

- The type of a node declares its params (part 04.3). The patch stores only the literal of each
  param that was set, and its source. Types, ranges and options always come from the
  declaration. A stored value that the type does not declare is ignored.
- A param, or one element of a vector param (`name[i]`), has exactly one active source: a
  **constant** or an **expression**. A literal written to a driven param makes it constant and
  keeps the expression text.
- The **zero point** of a node is the state that "non-default" is measured against. It can be
  set to the present state.
- A **pulse** param holds no value. It is an event input of the node in the form of a param
  (3.7): each firing is one event, and the hook of the pulse runs once for it. A pulse request
  fires it once. Its source is an event, or a gate whose rising edge fires.
- A **refresh** request asks a node to list the options of a param again (devices, ports,
  files).
- A pulse and a refresh are not edits: they change no state and make no history step.
- **Expressions** are Python expressions. An expression can read:
  - `nd('name')`: the first output of a node; `nd('name').out.<slot>`;
    `nd('name').params.<group>.<param>`;
  - `me`: the node the expression belongs to, in the same forms;
  - `vars.<group>.<element>`: a variable;
  - `sm('playhead').<attribute>`: a playhead of a state machine;
  - `t`: the patch time;
  - `np`, the math names, and the generators: `lfo(freq, src, vmin, vmax, fn)` with the shapes
    sine, cosine, triangle, square and saw; `nse(...)` noise; `qtz(v, scale, root, mode)`
    quantization to a scale in Hz or volts per octave; `gte(x, attack, decay, sustain, release)`
    an envelope gate with state per call site;
  - events, with the same terms: an event output, a pulse param, an event variable, an event of
    a machine. A read of an event is true exactly once per event (3.7.5).
- An expression that is one reference is a direct read: it costs what a cable costs and involves
  no interpreter. A direct read of an audio or texture output is legal for a numeric param of a
  node on the same engine, where it is modulation at the rate of that engine.
- A computed expression is evaluated before each run of its node and when its inputs change.
  When nothing arrived, on an error or on a non-finite result, the literal stays in effect.
- A result is a number, a list of numbers or a string. A list of the exact length on a vector
  param drives each element.
- An expression that reads only `t` and constants can travel as a functional and be evaluated at
  the instant of its consumer (an audio block, a render tick).
- An error of binding (an unknown node, slot or variable) stays with the source and shows under
  its row. An error of evaluation stays with the running node.
- A declaration can give a default expression. It is applied when a node is added with no
  params supplied.
- The backend reports, per node, the set of nodes and variables its expressions read.

## 3.5 Variables

- A variable is `group.element`. Its value is an array or a string. A variable can also be an
  event (3.7): it holds no value and it fires.
- The patch holds the configuration of a variable: its form, its control record (part 08.6), its
  expression and its locks. Values travel on the data plane as frames. A wide value is saved in
  the workspace; a small value is saved in the document. The limit is one stated value.
- A group has a configuration lock and a value lock. A group that has an owner (a MIDI device, a
  playhead, the system) is written only by its owner.
- A computed variable holds an expression.
- System groups, locked in configuration:

  | Variable | Default | Meaning |
  | --- | --- | --- |
  | `system.viewer_fps` | 30 | The highest rate at which viewers are served. |
  | `signal.default_ufreq` | 30 | The default rate cap of a producer on the signal engine. |
  | `graphics.default_width`, `graphics.default_height` | 1024 | The size of a texture with no other rule. |
  | `graphics.fps` | 60 | The rate of the graphics clock (1 to 240). |
  | `audio.buffer_size` | 256 | Frames per device callback; 0 is the default of the host. |
  | `record.name`, `record.root` | — | The defaults of a take. |
  | `audio.rate`, `audio.channels`, `audio.device`, `audio.driver`, `audio.hosts`, `system.goofi_home` | — | Published by the engines and the system; never saved. |

- **MIDI:** a controller comes in as a variable group with the entries `cc` (16 × 128), `notes`
  (16 × 128), `bend` (16) and `pressure` (16), and the events `note_on` and `note_off` (with
  channel, note and velocity). MIDI learn creates the group for a port. A
  `Variable` node of each engine puts a variable on a cable.
- The name of a variable, a group, a machine, a state, an attribute or a playhead is a letter
  or an underscore, then letters, digits and underscores.
- A rename of a variable, a group, an attribute, a state, a playhead or a port rewrites every
  expression that spells the old name. A rename of a variable can move it to another group.
- A computed variable is evaluated at most at the viewer rate. A value written by an expression
  or by an owner makes no history step and does not make the patch unsaved.
- A MIDI port is open only while the patch reads its group, or during a learn.

## 3.6 State machines

### 3.6.1 Model

- A patch can hold state machines beside its graph.
- A machine has: **attributes** (name, value description, default, easing curve), **states**
  (position, the values it sets for a subset of the attributes, an optional ADSR envelope per
  number attribute, an interval, a decision mode), **transitions** (from a state or from any
  state, to a state, an optional condition, a duration, optional values), **playheads** (name,
  colour, start state) and a seed.
- Easing curves: step, linear, in, out, in-out, smooth, and a cubic bezier.
- A playhead that enters a state takes the values the state sets and keeps the others. An
  envelope plays on arrival. Over the duration of a transition each attribute eases by its own
  curve.
- A state can set exit values, applied when a playhead leaves.
- A transition can be internal (from a state to itself without a new arrival).
- A duration or an interval can be a random range `{min, max}`, drawn once from the seed.
- An attribute cannot be named `state` or `progress`. A playhead cannot share a name with a
  state. A state is not removed while a playhead starts there.

### 3.6.2 Decisions

- At every interval after arrival (0 is once, −1 is never, or an interval expression), a state
  decides which transition out a playhead takes:
  - `sequence`: the next transition out, in an order authored on the state, remembered per
    playhead and state;
  - `random`: a weighted draw over the transitions out and "wait", from the seed;
  - `expression`: an index; a negative index waits.
- A state with no way out lands its values again.
- A transition with a condition is taken the instant the condition turns true. Other edge modes
  of a condition: `falling`, `change`, `level`.
- A transition can be triggered by a named event (`event {name}`) and can be `always`.
- Ops fire a transition, send a named event, jump a playhead to a state, and reset a machine.
  These are effects: they change no patch state and make no history step.
- Machine expressions are the expressions of 3.4, plus `me` (the playhead the expression is
  read for: its name, state, arrival and attributes) and `sm('state')` (its residents and its
  events; 3.6.4).
- `fire` takes only a transition that leaves the present state or the target state of the playhead.

### 3.6.3 Runtime

- Each playhead is a variable group written only by the machine: `<playhead>.state`,
  `<playhead>.progress` and one entry per attribute. A param reads it as
  `sm('playhead').<attribute>`.
- A value in flight is a functional of patch time, read by whoever consumes it. The machine
  never publishes periodically.
- The machine wakes only for: a landing, a due decision, a request, a replaced model, a write to
  a variable it reads, a frame of a node it reads.
- A rename of a playhead or of a state keeps the run of the renamed thing.
- A condition over a value in flight is observed when something happens (a write, an arrival,
  a due decision, a frame of a node it reads), not by polling.
- Several conditions true at one instant take the first in authored order. A condition goes
  before a decision. A chain of instant transitions is cut at a limit and logged.
- A changed model keeps the place and the values of each playhead; a playhead whose state is
  gone starts again.

### 3.6.4 Events of a machine

- A machine emits events as they happen (3.7). Per playhead: `arrived` (it lands in a state),
  `departed` (it leaves a state: a transition starts) and `due` (the interval of its state is
  due). Per state: `arrived` and `departed`, for any playhead. Per machine: the named events
  that the op `machine event` sends.
- An event of a machine carries the playhead, the state left, the state entered and the instant.
- The events are read as `sm('playhead').arrived`, `sm('state').departed` and so on. A read is
  true once per event for each reader (3.7.5). The events of a playhead are also event entries
  of its variable group, so a `Variable` node puts them on an event cable.
- A transition is taken by its condition, by an event (a named event of the machine, or any
  event that its condition reads), or always. A transition that waits for an event is taken once
  per event.
- What describes the present stays a value: `state`, `progress` and the attributes.

## 3.7 Events

An event is native to goofi. It has a kind of its own in frames, slots, cables, params and
variables, one delivery rule, and one way to be read. This section is the one home of these
rules; the other parts refer to it.

### 3.7.1 What an event is

- An event is something that happened once, at one instant: a tick of a clock, a note, a
  threshold crossed, a button pressed, a transition of a state machine. It has no duration and
  it leaves no value behind.
- A value says what is. An event says what happened. A reader of a value reads at its own rate
  and gets the newest value. A reader of an event gets each event exactly once.
- An event carries: its instant in patch time; its source (the stamps of part 02.6.2); its
  name; and a small payload (for a transition: the playhead, the state left and the state
  entered; for a note: channel, note and velocity).
- Two events are never merged into one. One event is never seen twice by one reader.

### 3.7.2 Where events come from

- **An event output of a node.** A node declares an output slot of kind event and emits on it:
  the tick of a clock, the edge of a threshold (rising, falling or both), a note of a MIDI
  input, the end of a playback.
- **A pulse param.** A pulse param is an event input in the form of a param (part 04.3). Each
  firing is one event. It is fired by its button, by the op `node param pulse`, or by its
  source.
- **A state machine** (3.6.4): arrivals, departures, due decisions and named events.
- **An event variable** (3.5): a button of a control panel; the note-on and note-off of a MIDI
  device group; the op `variable fire`.
- **A gate.** A source of a pulse param that gives a value, not an event, is a gate: one event
  occurs at the instant the value turns true. The edge can be rising (the default), falling or
  change.

### 3.7.3 Delivery

- Each listener gets each event once, in the order of the instants, whatever the rate of the
  listener and of the source.
- Each listener has a queue of its own. In the live mode the queue is bounded: when it is full
  the oldest event is dropped, the drop is counted, and it shows as a gap in the index of the
  stream and in the stats of the listener. In the Kahn mode no event is dropped (part 06.3).
- An event keeps its own instant. A listener that handles an event late still reads the instant
  at which it happened.
- An event crosses between the planes like the other wire kinds. On a plane with a clock (an
  audio block, a render tick) the events that are pending take effect at the start of the next
  block or tick. The plane reads their count for that block or tick, so none is merged and none
  is delayed further.

### 3.7.4 An event runs a node

An event acts on a node exactly once. There are three ways:

1. **An event input slot.** The default coupling of an event slot is `each` (part 06.1): the
   node runs once per event, and the run receives the event.
2. **A pulse param of the node.** The hook of the pulse runs once per event: a reset, a step, a
   capture.
3. **The common `trigger` param** (part 04.3). It is a pulse param of every node that schedules
   itself: each event runs the node once. A node needs no event slot to be run by an event.

- A node that runs for events runs once per event that it was sent, never once per tick on
  which it looks.
- The rate cap of a node does not merge events. Events above the cap wait in the queue of the
  listener.

### 3.7.5 Reading an event: true once

- An event is read wherever a value is read, with the same terms (3.4):
  `nd('name').out.<slot>` for an event output; `nd('name').params.<group>.<param>` for a pulse
  param; `vars.<group>.<entry>` for an event variable; `sm('playhead').<event>` and
  `sm('state').<event>` for a machine.
- The read is true exactly once per event for each reader, and false at every other read. A
  reader is one param source, one condition of a machine, or one `hold` slot of a node. Two
  readers of one event each get their own true.
- The read that is true also gives the event: its name, its instant and its payload.
- An event that a source reads causes one evaluation of that source, at the instant of the
  event. A reader that is idle misses nothing. A reader that is fast sees nothing twice.
- When events arrive faster than a reader reads, each pending event makes one read true, in
  order.
- An event that is the whole source of a pulse param is a direct read (3.4): the pulse fires
  once per event, without an interpreter.
- An event read inside a computed source gives that source one evaluation with true. The value
  it computes holds until the next evaluation.
- In the code of a node, a `hold` slot of kind event gives the next pending event once, and
  nothing when none is pending.
- On a plane with a clock the read gives the count of the events of the block or the tick, and
  zero in every other block or tick (3.7.3).

### 3.7.6 Events in the interface and on disk

- An event slot has a colour of its own (part 08.4.2). Its pill flashes when an event passes.
- The viewer of an event output is a raster of the recent events over time, with the name and
  the payload of the newest (part 08.8.2).
- The stats of a node count its events in and out per second, and its drops.
- The recorder records an armed event output as one row per event: instant, name and payload
  (part 02.9.4).
- A control panel has a button widget that fires an event variable (part 08.6).

### 3.7.7 Events and time

- In the live mode the instant of an event is the patch time at which its source emitted it.
- In the Kahn mode an event is a frame with an instant, like every other frame. A read at
  instant t takes the pending events with an instant at or before t, one per read. The result is
  the same on every run (part 06.3.2).

## 3.8 Arrangement and canvas placement

- The arrangement is a list of tabs. Each tab is a tree of splits (row or column, with size
  fractions) and panels (an identity, a panel type and its state).
- A new patch has one tab with one node editor.
- Ops: add, remove, rename and reorder a tab; split a panel; insert at a panel; move and remove a
  subtree; resize a split (the ratio is clamped to 0.05 to 0.95); set the type and the state of a
  panel. A change of type clears the state. The last tab stays.
- A corrupt arrangement costs the layout, not the patch: the patch loads with the default
  arrangement.
- The canvas has one grid. The footprint of a node follows from its slots and open viewers. A
  node without a position is placed to the right of the last one. Arrange lays out a scope by
  dataflow layers. Machines have a placement of states and a ring arrangement.
- The canvas constants are declared once and generated for the frontend.

## 3.9 The patch as a value

- A patch is an immutable value. A write copies the one record it touches and shares the rest.
- **Batch.** A set of ops is applied as one batch. One history step holds the snapshot from
  before the batch.
- **Settle.** After a batch, the difference between the two snapshots is read once: changed,
  removed, retyped and renamed nodes, moved cables, edited params, armed outputs, machines,
  arrangement. From that difference alone: nodes are born, reborn and halted; bindings are
  derived for the changed nodes and for the nodes that refer to them; each engine gets one
  settle with the changed set; the recorder gets the armed set. Nothing happens when nothing
  moved.
- The cost of a settle follows the size of the change, not the size of the patch.
- Births and removals of running nodes happen only at a settle.
- **History.** One history of snapshots serves every client. A write pushes the snapshot from
  before it and clears redo. Undo and redo swap a snapshot in and report which nodes changed.
  Steps with the same group token merge into one.
- **Preview.** A drag publishes previews without history steps. A concurrent write of another
  client is kept under the preview. The release commits one step. A closed connection, an undo
  or a redo ends the previews of that client.
- Several people edit one patch at once, on one history: undo takes back the last write,
  whoever made it.
- A request (pulse, refresh, machine effect, record start) is not a write and has no history
  step.

## 3.10 Replication

- The backend owns the patch. A client holds a read-only replica of the document.
- A new client gets the whole document. After each settle every client gets versioned path
  operations (`put`, `del`) for exactly the records that changed.
- A client never applies its own edit locally as truth. It can draw an edit in advance, and it
  removes the drawing when the op is refused.
- One change wakes only the readers of the record it touched.
- Runtime state reaches a client as events on the control connection: health and rates of nodes,
  evaluated param values and source errors, log batches, the status of a recording, the roster
  of agents. The presence of other clients has a connection of its own.
- Every vocabulary that both sides use (op names and arguments, panel types, viewer kinds, wire
  tags, events) is declared once in the backend and generated for the frontend.

## 3.11 Files

- A patch file (`.gfi`) is an archive of the document and the workspace. The document is text
  (YAML) with a format version and the program version. The pack is deterministic.
- An ignore file in the workspace excludes names, folders and extensions from the archive.
- A patch file carries the node files of the private bundle that the patch uses, so the patch
  opens on another machine. On load, a copy that equals the file of the library is dropped in
  favour of the library.
- A change of a file in the workspace makes the patch unsaved.
- A save writes beside the target and renames. A refused load leaves the open patch untouched.
- A patch remembers the file it was opened from or saved to. "Unsaved" is answered by a
  fingerprint of the file.
- Save, save as, open, new, and reset to an example are ops. "Download a copy" and "upload" pass
  the file through the browser and leave the remembered file alone.
- **Recovery:** the program keeps a recovery copy of an unsaved patch in the home. At the next
  start it offers to recover or discard.
- **Admission** of a loaded or pasted document: a type that no source offers is handled by the
  rule of part 04.8.4 (install, load without it, or cancel); a type that is known but
  unavailable stays in the patch (3.2); unknown params and dangling cables are dropped with a
  warning; an illegal variable name refuses.
- A clipboard fragment and a patch are one format. A fragment holds nodes and the cables among
  them.
- The state of a node that the node owns (the state of a VST3 plugin, a painted image) is saved
  in the workspace per node at save and at removal, and read at birth.
- The shipped examples open from a menu and from the CLI.
- A load or a new patch clears the history, stops a running take and stops the agents of the
  replaced workspace.
- A file of another format version is refused, and the refusal names the version that wrote it.
  A pack refuses two names that are equal when case is ignored.
- A node that could not be built keeps its saved state. The saved state of a node that the patch
  no longer holds is removed.

## 3.12 Open decisions

1. Whether an audio node consumes an event at the block or at the sample.
2. How much an event carries beyond its name, its instant and its source: whether a transition
   also carries the attribute values of the instant.
3. Exact step onsets in audio: a step curve sounds up to one block late. A presentation delay is
   not to be built unless a patch needs it.
4. The limit between a variable value saved in the document and one saved in the workspace.
5. The terms of 3.7.5 read an event like a value. The present roadmap (`triggers.md`) names a
   separate form, `nd().on(...)`. One form is to be confirmed.
6. The depth of the event queue of a listener in the live mode.
