# Timeline

A timeline panel: tracks of notes and of keyframed curves, stacked on one time axis, each track
a group of variables the patch reads. The clock it plays on shipped: the position of
`goofi_supervisor::timing::Time`, the `clock` group and `clock set`. This file holds the
decisions for what remains and the order of the work.

## Decisions

### Tracks

- One timeline per patch, a document root. Every timeline panel shows the same tracks.
- A track has an id and a name, as a machine's records have: a rename moves no key, and the
  track's group, which its name names, moves with what it holds.
- Two track kinds: `note` and `curve`.
- A track's times are in its own unit, `seconds` or `beats` of `clock.bpm`. A panel picks the
  unit of its ruler.
- A `curve` track holds attributes, each a number in a range, each with keyframes. A keyframe
  sets the value at its time and names the curve (`goofi_core::ease::Curve`) to the next
  keyframe. The value holds before the first keyframe and after the last.
- A `note` track holds notes: start, length, pitch, velocity.
- A track is an owned group (`Owner::Track`): `vars.<track>.<attribute>` for a curve track,
  `vars.<track>.notes` for a note track, in the shape a MIDI device's `notes` has
  (`goofi_core::variables::MIDI_ENTRIES`, channel 1), so a reader of a device reads a track.
- A curve attribute is ONE functional of `t` over the whole track, written again when the track
  or the tempo moves. No thread wakes for a keyframe.
- The ops are `timeline list` and `timeline track add`, `edit` and `remove`. An edit replaces a
  track's notes, or an attribute's keyframes, whole.

### Shared with the state machine

- The timeline builds its curves from the machine's blend (`Motion`, `Leg`, the functional's
  source).
- The thread that parks toward the next instant owed, takes its inbox and drives the store
  (`goofi_bridge::machines`) serves both. No second copy of the loop.

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
- The panel shows and drives the clock: play, pause, stop, the position, the tempo, the length
  and the loop. It owns no transport of its own.

## Remaining work

In this order, each a tested commit:

1. The timeline root, the `timeline` ops, the track groups, the curve functionals, the thread.
2. Note tracks: the store's `notes`, the queue, the `MidiIn` port.
3. The panel: tracks, show and hide, the menu, the note editor, the keyframe and curve editor,
   the clock controls.
4. The words: the architecture lines of AGENTS.md (time, queues, the timeline), a README
   section, the orientation.

## Open

- What a machine does when the position goes back (a jump, a restart at the length). Today a
  position that went back starts every run over, as a load does; that rule holds until decided.
- A node that measures elapsed time from its `now` (`Clock`, the simulations) sees a negative
  step when the position goes back. No node was changed for it.
- Whether a note track also publishes the newest note as `pitch`, `gate` and `velocity`.
- How `MidiIn`'s `port` param holds a track: by id, so a rename of the track needs no rewrite.
- Whether a variable gets an id too. Today its name is its key and a rename rewrites its readers.
- The `Clock` node keeps a tempo of its own. Whether it reads `clock.bpm` instead.
- The expression editor still starts a completion behind a dismissed window, and Tab takes it.
