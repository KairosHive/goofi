# Timeline

Shipped: the clock the timeline plays on (the position of `goofi_supervisor::timing::Time`, the
`clock` group, `clock set`), the tracks (`goofi_graph::timeline`, the `timeline` ops, a track's
group, written by the machines' thread), a note track as a port of the audio `MidiIn` node
(`goofi_audio::TrackPorts`) and the panel (`TimelinePanel.svelte`). This file holds what is open.

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
- The panel's axis fits the clock's length, or else the tracks' content: it has no zoom, no
  scroll and no snap to a grid.
- The panel edits one note or keyframe at a time: no selection of several, no copy.
- The panel has no keyboard path to a note, a keyframe or a track's menu.
- A name the manager refuses, or one that is no identifier, stays in the name field until the
  name next moves: the shared live value holds a commit as pending. The machine's attribute
  form has the same fault.
- A panel keeps the id of a removed track in `state.hidden`; nothing reads it.
- `MidiIn`'s `port` param holds a track by its name (`timeline: <name>`), so a rename of the
  track leaves the node on a port nothing plays. Whether it holds the id.
- Whether a variable gets an id too. Today its name is its key and a rename rewrites its readers.
- The `Clock` node keeps a tempo of its own. Whether it reads `clock.bpm` instead.
- The expression editor still starts a completion behind a dismissed window, and Tab takes it.
