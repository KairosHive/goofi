# Timeline

A timeline panel: tracks of notes and of keyframed curves, stacked on one time axis, each track
a group of variables the patch reads. Shipped: the clock it plays on (the position of
`goofi_supervisor::timing::Time`, the `clock` group, `clock set`) and the tracks themselves
(`goofi_graph::timeline`, the `timeline` ops, a track's group, written by the machines' thread).
This file holds the decisions for what remains and the order of the work.

## Decisions

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
- A track's id is never minted twice, so the id of a removed track in a panel's state names
  nothing: a panel passes over an id it does not know, and nothing rewrites the panels.
- The panel shows and drives the clock: play, pause, stop, the position, the tempo, the length
  and the loop. It owns no transport of its own.

## Remaining work

In this order, each a tested commit:

1. Note tracks as events: the queue, the `MidiIn` port. (`vars.<track>.notes` shipped.)
2. The panel: tracks, show and hide, the menu, the note editor, the keyframe and curve editor,
   the clock controls.
3. The words: the architecture lines of AGENTS.md (queues, the timeline), a README section,
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
- Whether a note track also publishes the newest note as `pitch`, `gate` and `velocity`.
- How `MidiIn`'s `port` param holds a track: by id, so a rename of the track needs no rewrite.
- Whether a variable gets an id too. Today its name is its key and a rename rewrites its readers.
- The `Clock` node keeps a tempo of its own. Whether it reads `clock.bpm` instead.
- The expression editor still starts a completion behind a dismissed window, and Tab takes it.
