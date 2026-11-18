# 08 (continued). goofi-pipe: the other panels

This file holds sections 8.8 to 8.15.

## 8.8 Viewers and the viewer panel

### 8.8.1 Two hosts, one viewer

- A viewer shows the live frames of one output slot. It has two hosts: the inline viewer on a
  node card (part 08.4.2) and the viewer panel.
- A view is one record: the viewer kind and its settings. The inline viewer stores it on the
  node; the viewer panel stores it in its panel state. Both are validated by one schema.
- The viewer panel is bound to a node (8.3.3). It has a slot select, the kind select and the
  settings menu.
- The viewer components belong to the shared UI library (part 09.5).
- A slot that does not exist falls back to the first output.

### 8.8.2 Kinds

| Kind | Shows | Modes |
| --- | --- | --- |
| line | Arrays of 0 to 2 dimensions. | `series`: one trace per row. `trajectory`: each pair of rows as a path in the plane. |
| image | Arrays of 2 or 3 dimensions with 1 to 4 channels. 1 or 2 channels go through a colormap; 3 or 4 are colour; alpha shows over a checkerboard. | — |
| brain | EEG arrays with channel names. | `topomap` (one value per channel, interpolated on a head), `ring` and `3d` (a matrix between channels as edges), `auto`. |
| string | Strings. | Plain text or sanitized Markdown. |
| table | Tables. | A tree of keys; nested tables collapse. |

- The kind follows the slot: a string slot shows the string viewer and a table slot the table
  viewer. An array slot defaults to line, an audio slot to line, a texture slot to image, and
  the user can choose line, image or brain.
- An image with a log scale and a colormap is a spectrogram. A texture output is shown from the
  readback of the graphics engine.
- A topomap needs at least three channel names of the 10-20 layout. A ring and the 3D view need
  names on both axes. The 3D view turns by a drag; its camera is not stored.
- A frame the kind cannot draw shows its shape, dtype and its minimum, maximum and mean. Each
  failure to draw has a message that says what is missing.
- Empty states: no output slots; no data yet; the browser cannot draw.
- Channel names match without case and include the older names T3 to T6, A1 and A2. A square
  matrix with names on one axis uses them for both. A ring needs two known channels.

### 8.8.3 Settings

- line: mode; log x; log y; auto range, minimum, maximum; show points; point size; thickness.
- image: colormap; auto range, minimum, maximum; log scale; stretch to fill or keep the aspect.
- brain: mode; colormap; range; contour lines; electrodes; the share of edges shown; curve.
- string: Markdown; word wrap. table: decimals.
- Colormaps: viridis, magma, plasma, gray, jet, coolwarm.
- A setting shows only when the resolved mode uses it.

### 8.8.4 Reading a viewer

- Corner labels show the range of a line plot.
- A hover, or a finger held on the viewer, reads the nearest value: a vertex with its series
  name, an image cell with its channels, an electrode, an edge with its strongest partner. Axis
  names come from the channel labels of the frame.
- The inspector shows the stats of a stream: arrivals and drops per second.
- The corner labels show the y maximum, the y minimum and the x maximum: on hover, on a selected
  node, on touch, and always in a viewer panel.

### 8.8.5 Drawing

- Decoding and drawing run off the main thread. The main thread gets a frame only where a
  reader asked for it.
- All viewers of an editor draw on one surface. One failed draw does not stop the others.
- A viewer subscribes only while it is in the viewport, and asks for no more than its pixels
  (part 02.10).
- Viewer colours come from the tokens of the theme.

## 8.9 The state machine panel

The panel edits the machines of the patch (part 03.6). It has a canvas and a score.

### 8.9.1 Bar

- Tabs of the machines: select, add, rename, remove. A seed field and a reset button.
- To remove the last machine adds a new one in the same step.

### 8.9.2 Canvas

- A state is a card on a grid. It shows its name, the attributes it sets, its mode, its
  interval and the counts of its transitions.
- A transition is a line with a direction mark; a self transition is a loop; parallel
  transitions have lanes. A selected transition shows its share, turn or index and its
  duration.
- Playheads show as dots: docked on the card they rest in, in arrival order, or flying along a
  transition at its progress.
- Gestures: a double click, a double tap or a long press adds a state. A drag from the border
  of a card adds a transition. With a selected group the drag fans out or fans in, or asks for
  all to all. A chain tool wires every card it crosses.
- Card menu: all to all; chain by position; ring by position; add a playhead; delete.
- Controls: arrange on a ring; the chain tool; add N states, chained or not.
- Keys: Escape, Delete, copy, duplicate, select all, paste. A copied fragment keeps everything a
  machine holds, weights included.
- A card drag previews and commits one op.
- A card dragged onto a param row binds the row to that attribute of the playhead.
- Alt held during a drag acts as the chain tool.

### 8.9.3 Score

- The score shows the whole machine as columns and rows in a side pane: a column per state in
  loop order, as wide as its interval; a row per attribute; every cell a slider.
- A drag paints across columns, so a melody is drawn over the states. A drag over selected
  columns edits them as one step.
- Cell faces: number, vector, colour, envelope, toggle, text, select.
- A lane above shows the transitions as arcs. A drag of a column edge sets the interval. Sweep
  lines show the playheads and the time to the next decision.
- The foot adds an attribute. A plus appends a state with a transition from the last.
- Settings behind a cog on each column, row and cell:
  - cell: the value, unset, the envelope and its shape;
  - attribute: name, kind, curve, range, integer, options, default, remove;
  - state: name, mode, the order of its transitions, index expression, interval or its
    expression, exit values, playheads (colour, name, start, remove), transitions out with
    linked share sliders that include "wait", transitions in;
  - transition: fire, delete, condition and its edge mode, event, duration, values, internal.
- Several selected states are edited as one; differing values are marked as mixed.
- Hover and selection are linked between the canvas and the score.
- The position of a playhead is derived in one place for both views.

## 8.10 The recorder panel

- The panel arms outputs and runs takes (part 02.9). Arming is part of the patch: undoable,
  saved, copied with the node.
- Arm from an add menu or by a node dropped on the panel. A node with several outputs opens a
  checklist. All chosen slots are one history step.
- Fields: the name and the root of the take. They are the variables `record.name` and
  `record.root`; the default root is the recordings folder of the home.
- The recorder type of the take: files, or a dataset.
- Start and stop. Start is refused when nothing is armed or a take runs, with the reason.
- Status: a running mark that warns on an error, the elapsed time, the folder. Per stream: its
  state, `node/slot`, its format, frames written, the fill of its queue, a disarm control, and a
  quality select for a texture stream.
- Playback of a recording is by the playback nodes, not by the panel.

## 8.11 The console panel

- The console shows the log (part 02.5): one row per group with a source mark, the time, the
  text, the repeat count and a copy control. A click on the source selects the node.
- Filters: level chips (information, warning, error); a text filter over source and text; a node
  filter by a list or by a dropped node.
- The list is virtualized. It stays at the bottom until the user scrolls up; a control returns
  to the bottom.
- **The prompt** runs the command lines of the CLI without the program name (part 07.2.2). Up
  and Down walk the history; Tab completes: one match is inserted, several are listed. The
  command and its result are written to the log. The prompt uses the control connection like
  every other part of the app, and its ops land on the one history.
- Filters and the history of the prompt are part of the viewpoint of the client (8.3.4).

## 8.12 The agent panel

- The panel is a terminal on an agent instance (part 07.3).
- With no instance it shows a launcher: one tile per running instance to attach to, and one
  tile per harness of the configuration, with its model and effort options. It says why when
  there is nothing to launch.
- With an instance it shows tabs of the instances over a terminal with scrollback and the
  colours of the theme.
- The terminal and its connection live outside the panel. Keys typed during the connection
  are kept.
- Closing a view asks: detach (the harness keeps running), kill, or cancel.
- An unexpected exit is a notification.
- A free panel shows an instance that no other panel shows.
- A detached panel stays on its launcher.

## 8.13 Plugin panels

- A plugin registers panels and header entries (part 07.4.3). A plugin panel is added, moved and
  closed like any panel. It can be a drop target for nodes, with the same drop hint as a
  built-in panel.

## 8.14 The library view

- The library view (part 04.8.3) shows the sources, the bundles with their state of
  preparation, and the entries with their docs and params. It installs, updates and removes
  bundles, and searches the online library.

## 8.15 Open decisions

1. Whether a topomap value needs a second narrow depth on the wire; measure the visible
   difference first.
2. Bandwidth to a tablet over a LAN is not measured. Generic compression of frames is not to be
   built (it gains 1.05 to 1.4 times on such data); an overlap between frames is never
   inferred.
3. Whether the library view is a panel type or a dialog.
