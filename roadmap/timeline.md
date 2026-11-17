# Timeline

A timeline panel: tracks of notes and of keyframed curves, stacked on one time axis, each track
a group of variables the patch reads. Shipped: the clock it plays on (the position of
`goofi_supervisor::timing::Time`, the `clock` group, `clock set`), the tracks themselves
(`goofi_graph::timeline`, the `timeline` ops, a track's group, written by the machines' thread)
and a note track as a port of the audio `MidiIn` node (`goofi_audio::TrackPorts`).
This file holds the decisions for what remains and the order of the work.

## Decisions

### Panel

- Panel type `timeline`. Tracks are stacked vertically on one time axis.
- A panel's state holds the ids of the tracks it hides. Two panels can hide different sets.
- A hidden track is listed in a dropdown in the panel's bar; a click on it shows it again.
- Each track has a hide button. A right click on a track, or a long press, opens a menu:
  `Hide Track`, `Hide Tracks Above`, `Hide Tracks Below`, `Hide Other Tracks`, `Delete Track…`.
  The delete asks first (`ConfirmDialog`).
- A track's id is never minted twice, so the id of a removed track in a panel's state names
  nothing: a panel passes over an id it does not know, and nothing rewrites the panels.
- The panel shows and drives the clock: play, pause, stop, the position, the tempo, the length
  and the loop. It owns no transport of its own.

## Remaining work

In this order, each a tested commit:

1. The panel: tracks, show and hide, the menu, the note editor, the keyframe and curve editor,
   the clock controls.
2. The words: the architecture lines of AGENTS.md (the timeline, its ports), a README section,
   the orientation.

## Open

- What a machine does when the position goes back (a jump, a restart at the length). Today a
  position that went back starts every run over, as a load does; that rule holds until decided.
- A node's `now` is the run time, so a node runs on through a pause and never sees time go
  back; a node that follows the position reads `t` in an expression. Whether a node gets the
  position too.
- A curve attribute's functional nests one conditional per keyframe: the interpreter refuses
  to compile some thousands of them. Whether a lane gets a ceiling or a flat table.
- A control panel that draws a playhead's or a track's group is not re-aimed when the owner is
  renamed.
- A note reaches `MidiIn` at the block after the machines' thread wakes for it, as a device's
  note does. Whether the thread sends a note ahead, stamped with the instant it falls on, so
  the node lands it in the block that holds that instant.
- Two notes of one pitch that abut end and start in one block, so a `MidiIn` of one voice
  keeps its gate up and plays them as one note.
- A note is known by its place in the track, so a note taken out before one that sounds
  strikes the sounding one again.
- A note ring that is full drops the note, an end included. Not examined: whether the audio
  thread can stop draining a node's ring while the node is in the plan.
- Whether a note track also publishes the newest note as `pitch`, `gate` and `velocity`.
- `MidiIn`'s `port` param holds a track by its name (`timeline: <name>`), so a rename of the
  track leaves the node on a port nothing plays. Whether it holds the id.
- Whether a variable gets an id too. Today its name is its key and a rename rewrites its readers.
- The `Clock` node keeps a tempo of its own. Whether it reads `clock.bpm` instead.
- The expression editor still starts a completion behind a dismissed window, and Tab takes it.
