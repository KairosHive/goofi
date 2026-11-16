# Timeline

A timeline panel: tracks of notes and of keyframed curves, stacked on one time axis, each track
a group of variables the patch reads. Nothing is built. This file holds the decisions and the
order of the work.

Build needs on Linux: `pkg-config`, `libasound2-dev` and the JACK development library (the
build asks for `jack.pc`). `goofi-bridge` and `goofi-tests` do not build without them.

## Decisions

### Identity

- An object has an id that the manager mints; its name is an attribute of the id-keyed record,
  as a node has a uid and a name. This holds for a machine, a state, a playhead, an attribute
  and a track.
- A rename changes the name and the texts that spell it (expressions, as a node rename does).
  It changes no key, so no run, flight, cursor, gate or panel state moves.
- Every runtime structure and every panel state keys by id. A panel never holds a name.
- Ops take a name where a person types one and resolve it to the id at the door, as node ops do.
- A variable group keeps its name as its key: the name is what an expression spells. An owned
  group follows its owner's rename as a move of the group, with its values and wires kept.

### Group owners

- A group has one `owner`: `midi` (a device), `machine` (a playhead) or `track`. It replaces the
  `midi` and `machine` fields of `goofi_core::variables::Group`.
- A control panel's group is not owned: any caller writes it.

### Clock

- `goofi_supervisor::timing::Time` has two readings, on one object.
  - The run time: seconds since the load, monotonic. The stamps (`procstart`, `procend`), the
    recorder, the audio block ties, a fault's `since` and every wait read it.
  - The position: it plays from the load, and it can pause, resume, stop, jump to any time and
    restart at a length. `t` in an expression, a node's `now`, a functional, the machines, the
    timeline and a shader's time read it.
- Pause stops the position alone. The engines, the nodes and the devices run on.
- The default is endless: no length, the position counts up as it does today.
- The clock is not a node. It is the group `clock`, one of goofi's own groups:
  - authored, saved with the patch: `clock.bpm`, `clock.length` (seconds), `clock.loop`
    (whether the position restarts at the length);
  - derived, never saved: `clock.time`, `clock.beat`, `clock.progress`, `clock.playing`.
- The derived three that move are functionals of `t`, written when the clock's state moves.
  Nothing publishes them periodically.
- `clock play`, `clock pause`, `clock stop` and `clock seek` are effects: no undo step, no dirty
  mark. The tempo, the length and the loop flag are written as any system variable is.
- The timeline panel shows and drives the clock. It owns no transport of its own.

### Tracks

- One timeline per patch, a document root. Every timeline panel shows the same tracks.
- Two track kinds: `note` and `curve`.
- A track's times are in its own unit, `seconds` or `beats`. A panel picks the unit of its ruler.
- A `curve` track holds attributes, each of a kind a machine attribute has
  (`goofi_graph::machine::AttributeKind`), each with keyframes. A keyframe sets the value at its
  time and names the curve (`goofi_core::ease::Curve`) to the next keyframe. The value holds
  before the first keyframe and after the last.
- A `note` track holds notes: start, length, pitch, velocity, channel.
- A track is an owned group named by the track: `vars.<track>.<attribute>` for a curve track,
  `vars.<track>.notes` for a note track, in the shape a MIDI device's `notes` has
  (`goofi_core::variables::MIDI_ENTRIES`), so a reader of a device reads a track.
- A curve attribute is ONE functional of `t` over the whole track, written again when the track
  or the tempo moves. No thread wakes for a keyframe.

### Shared with the state machine

- The blend of a value along a curve (`Motion`, `Leg`, the functional's source) moves out of
  `goofi_graph::machine` into one module that the machine and the timeline both use.
- The thread that parks toward the next instant owed, takes its inbox and drives the store
  (`goofi_bridge::machines`) serves both steppers. No second copy of the loop.

### Notes as events

- A note track is a port of the audio `MidiIn` node, listed with the host's ports.
- The timeline sends each note ahead of its time, stamped with the run-time instant it falls
  on. The node lands it in the block that holds that instant, so the error is one block.
  A device's note lands at the next block, as today.
- The notes ride a transport queue per listening node: bounded, oldest dropped, never blocking
  the sender. AGENTS.md says that only the recorder reads a queue; that line changes in the
  same commit.
- A pause, a jump and a restart release every held note.
- Nothing more of `roadmap/triggers.md` is built here.

### Panel

- Panel type `timeline`. Tracks are stacked vertically on one time axis.
- A panel's state holds the ids of the tracks it hides. Two panels can hide different sets.
- A hidden track is listed in a dropdown in the panel's bar; a click on it shows it again.
- Each track has a hide button. A right click on a track, or a long press, opens a menu:
  `Hide Track`, `Hide Tracks Above`, `Hide Tracks Below`, `Hide Other Tracks`, `Delete Track…`.
  The delete asks first (`ConfirmDialog`).
- The removal of a track takes its id out of every panel's state in the same write. One
  function on `Layout` rewrites the state of every panel that names a thing; `unbind`,
  `retarget` and this are its three callers.

## Remaining work

In this order, each a tested commit:

1. Identity in the state machine: the model, the ops, the thread, the document, the panel and
   the tests. The fixture: a machine, a state and a playhead renamed in flight keep the run.
   It closes the rename item of `roadmap/state-machine.md`.
2. The `owner` field of a group.
3. The clock: the two readings of `Time`, each reader moved to the one it means, the `clock`
   group and the `clock` ops.
4. The shared blend module and the shared stepper thread.
5. The timeline root, the `timeline` ops, the track groups, the curve functionals.
6. Note tracks: the store's `notes`, the queue, the `MidiIn` port.
7. The panel: tracks, show and hide, the menu, the note editor, the keyframe and curve editor,
   the clock controls.
8. The words: the architecture lines of AGENTS.md (time, queues, the timeline), a README
   section, the orientation, the generated frontend files.

## Open

- What a machine does when the position goes back (a jump, a restart at the length). Today a
  clock that went back starts every run over, as a load does; that rule holds until decided.
- Whether a note track also publishes the newest note as `pitch`, `gate` and `velocity`.
- How `MidiIn`'s `port` param holds a track: by id, so a rename of the track needs no rewrite.
- Whether a variable gets an id too. Today its name is its key and a rename rewrites its readers.
- The `Clock` node keeps a tempo of its own. Whether it reads `clock.bpm` instead.
