# Timeline

Shipped: the clock the timeline plays on (the position of `goofi_supervisor::timing::Time`, the
`clock` group, `clock set`), the tracks (`goofi_graph::timeline`, the `timeline` ops, a track's
group, written by the machines' thread), a note track as a port of the audio `MidiIn` node
(`goofi_audio::TrackPorts`) and the panel (`TimelinePanel.svelte`). This file holds what is open.

## Open

- What a machine does when the position goes back (a jump, a restart at the end of the loop). Today a
  position that went back starts every run over, as a load does; that rule holds until decided.
- A node's `now` is the run time, so a node runs on through a pause and never sees time go
  back; a node that follows the position reads `t` in an expression. Whether a node gets the
  position too.
- A curve track's functional nests one conditional per keyframe: the interpreter refuses
  to compile some thousands of them. Whether a curve gets a ceiling or a flat table.
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
- The panel has no snap to a grid, and its view zooms by the range bar alone: no wheel and no
  pinch.
- The panel's view follows the clock by a page; whether it scrolls with it.
- A curve track's variable is `vars.<track>.value`, because every variable is in a group.
  Whether a track's value takes the track's name alone.
- A position put past the end of the loop folds into it; whether it plays on outside it.
- The panel edits one note or keyframe at a time: no selection of several, no copy.
- The panel has no keyboard path to a note, a keyframe or a track's menu.
- Under a finger a note is one row high: it takes no hit-size press, because the rows beside
  it are notes too.
- A drag writes its lane whole from the panel's replica, so an edit of that lane whose echo
  has not come back when the drag begins is overwritten.
- A name the manager refuses, or one that is no identifier, stays in the name field until the
  name next moves: the shared live value holds a commit as pending. The machine's attribute
  form has the same fault.
- A panel keeps the id of a removed track in `state.hidden` and `state.sizes`; nothing reads it.
- The loop's section is drawn again to change it: it has no edges to drag and cannot be moved.
- The range bar stands for everything from 0 to the view, so far along the axis a small drag
  of it is a long time.
- A track's scale is what its keyboard marks and what a gesture lands on. A note set by an op
  is kept off the scale, and a scale that is not of twelve tones is marked to the nearest key.
- In a standing panel a track says what it is in a box of one size: what does not fit scrolls.
- `MidiIn`'s `port` param holds a track by its name (`timeline: <name>`), so a rename of the
  track leaves the node on a port nothing plays. Whether it holds the id.
- Whether a variable gets an id too. Today its name is its key and a rename rewrites its readers.
- The `Clock` node keeps a tempo of its own. Whether it reads `clock.bpm` instead.
- The expression editor still starts a completion behind a dismissed window, and Tab takes it.
