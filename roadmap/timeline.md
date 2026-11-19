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
- A curve track's functional nests one conditional per keyframe, and the interpreter refuses
  to compile some thousands of them: a curve of 4000 fitted keyframes played in three probe
  runs, and one of 8000 did not in two. A recording that would leave a curve more than two
  thousand is refused, and the take is lost; an edit by an op has no such ceiling, and a curve
  past what compiles plays nothing and says nothing. Whether a curve becomes a table a reader
  looks up. A curve of thousands of keyframes is compiled again on every edit of any track.
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
- A curve track's variable is `vars.<track>.value`, because every variable is in a group.
  Whether a track's value takes the track's name alone.
- What a box picks is moved, set and taken out together; it is not copied, and no key deletes it.
- The panel has no keyboard path to a note, a keyframe or a track's menu.
- Under a finger a note is one row high: it takes no hit-size press, because the rows beside
  it are notes too.
- A drag writes its lane whole from the panel's replica, so an edit of that lane whose echo
  has not come back when the drag begins is overwritten.
- A name the manager refuses, or one that is no identifier, stays in the name field until the
  name next moves: the shared live value holds a commit as pending. The machine's attribute
  form has the same fault.
- A panel keeps the id of a removed track in `state.hidden` and `state.sizes`; nothing reads it.
- The range bar stands for everything from 0 to the view, so far along the axis a small drag
  of it is a long time.
- A track's scale is what its keyboard marks and what a gesture lands on. A note set by an op
  is kept off the scale, and a scale that is not of twelve tones is marked to the nearest key.
- A node cannot be dropped on a curve's input while that is in the track's menu, as it is in a
  standing panel. A press on the expression editor's completion window may close that menu
  before the completion is taken; not tried.
- A track's edge held at an end of the track area scrolls the area. A note or a keyframe dragged
  there does not.
- The edge-held resize has a fixture in a lying panel alone; a standing one is not tried.
- `MidiIn`'s `port` param holds a track by its name (`timeline: <name>`), so a rename of the
  track leaves the node on a port nothing plays. Whether it holds the id.
- Whether a variable gets an id too. Today its name is its key and a rename rewrites its readers.
- The `Clock` node keeps a tempo of its own. Whether it reads `clock.bpm` instead.
- The expression editor still starts a completion behind a dismissed window, and Tab takes it.
- Two fingers on a touch screen neither move the panel's view nor zoom it: a wheel does both, and
  the range bar. The Shift key's turn of a wheel is tried in Chromium alone.
- A picked note's settings are a menu on the note, and a picked keyframe's a section of the
  track's settings. Whether a keyframe gets the menu too.
- The clock's section is drawn again to change it: it has no edges to drag and cannot be
  moved. A position put past its end folds into it.
- A track dragged onto a param writes the expression that reads it; no test drops one there.
- A track dragged over the others shows no line where it will land.
- A section that lies past the timeline's length is taken as it is: the position leaves the
  length for it.
- What a box picks moves in time as one; in pitch each note lands on its own scale and keyboard,
  so a pick pushed past the keyboard's edge closes up. A box has no door under a finger.
- A track is lifted by the free edge of what it says about itself; under a finger the browser
  may take the press for a scroll. Not tried on a device.
- `in_out` made custom is one bezier near the preset's shape, not the shape itself.
- A recording reads a device's `notes` as any reader does, the newest value alone: a key that
  goes down and up between two looks of the machines' thread is no note, and two strikes of one
  pitch with no look between are one. Whether a recording takes the device's events instead.
- A recorded note is where it was played: it is not put on the track's grid or scale.
- A curve recording replaces the keyframes of the stretch it covered; it has no way to add to
  them, and no way to keep one pass of a loop and drop another.
- An input that reads no node holds its value between two changes, so a recording of it steps;
  one that reads a node is joined from frame to frame. The fit is to within a hundredth of the
  track's range, and the tolerance is not a setting.
- The fit of a take is slow where its cuts fall one sample apart. It runs with nothing held,
  but for a track that was edited while it ran: then it is made again in the write.
- A track that goes while its edge is held leaves the size the drag had until the drag ends.
- A recording whose input cannot be read, an unset variable or a node with no frame yet, shows
  as recording: the reason is in the log alone, and the end changes nothing.
- A recording ends when it is told to: a patch that is closed, or a track that is removed,
  drops what the recording took, and so does a write of the take that is refused.
- No recording was made from a MIDI device: the test host has no sequencer. The fixture drives
  a device's group by hand.
- The panel's record button plays a paused clock; the op leaves the clock alone. A recording
  has no count-in and no metronome.
- A range bar's end is dragged outward to fold: a bar that ends at the window's edge has no room
  for that drag, and the view is then widened by the other end or by a longer timeline.
- A section of a track's settings is unfolded or folded for as long as its panel shows the track:
  the panel's state does not keep it.
- A curve's input that is a number alone is the row's constant, which records a level line.
- What a recording has taken shows in the lane while it runs, from `vars.<track>.take`: the
  newest 256 notes, or 256 samples spread over the take, so a long take is drawn more coarsely
  than it is kept. A note recording was not seen in a browser: the test host has no device.
- A keyframe a drag paints is every 16 pixels along the axis, whatever the view.
