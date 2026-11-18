# 08 (continued). goofi-pipe: the patch editor

This file holds sections 8.4 to 8.7: the node editor, the inspector, the control panel and the
variables panel. Section 8.3 defines the workspace they live in. Part 09 defines the primitives
they are built from. Values such as pixel sizes are the present design values; they are known to
work and can change with the design.

## 8.4 The node editor panel

### 8.4.1 The panel

- An editor panel shows one scope of the patch: the root, or the inside of one sub-patch.
- Several editor panels can be open. Each owns its camera, its selection and its scope path.
  Only the active panel takes keys, paste and slot clicks.
- The camera of a panel survives a remount of the panel. Zoom limits are 0.05 to 4.
- The background is a dot grid. The grid pitch (24 px) is one slot unit and is the same constant
  the backend uses to place and arrange nodes.
- An empty scope shows an empty state with the hint to double-click or press Tab.
- A corner cluster holds: zoom in, zoom out, fit, arrange. It shows on hover or focus, and always
  on touch.
- Arrange lays out the entered scope by its dataflow, in one undo step. The camera does not move.
- A node added without a position lands on the grid to the right of the node added before it.

### 8.4.2 The node card

- A card has a fixed width, a header and a body. The body height follows the number of input
  slots and the stack of output viewers.
- **Header:** a status dot, the name, then the boot label or the update rate.
  - Dot states: running, starting ("creating…", "setting up…"), error, off. The tooltip of an
    error dot is the error text.
  - The rate reads `12.4 upd/s`. It is faint at rest and clear on hover, on selection and on
    touch.
  - The header is tinted with the colour of the engine of the node: signal, audio, graphics, and
    a colour for VST3 plugins.
- **Inputs:** one pill per input slot on the left border. The colour of a pill is the colour of
  its data type (array, string, table, audio, texture, event). A filled pill takes one source; a
  hollow pill takes a list of sources. The name of a slot shows on hover, on focus, and while a
  cable in flight is near.
- **Coupling bar:** a vertical bar on the left side holds one control per input slot that sets
  its coupling (`each`, `zip` and its group, `hold`; part 06.4).
- **Outputs:** one pill per output slot on the right border, labelled in the colour of its type.
- **Inline viewers:** each output slot has a viewer in the body: a collapse control, a viewer
  kind select, a settings control, the slot name, and the plot. A viewer is open by default and
  collapsed by default on a node with three or more outputs. The open state and the viewer
  settings are part of the node record. All plots of a canvas draw on one shared surface under
  the cards.
- **Variable chips:** above the card, one chip per variable that an expression of the node
  reads.
- **Reference lines:** dashed lines, not cables, join the selected node to each node it reads
  through an expression, and to each node that reads it.
- **States of a card:** selected, error, booting, ghost (during placement), and a flash on the
  nodes that an undo or a redo touched.
- The set of nodes an expression reads comes from the backend; the browser does not parse
  expressions.

### 8.4.3 Cables

- A drag from a pill to a pill adds a cable. The backend decides compatibility. A refusal shows
  a toast and the cable drawn in advance is removed.
- An input takes one source, or a list when the slot is a list slot. An output fans out.
- To move an endpoint of a cable is one undo step.
- A cable is neutral at rest, tinted on hover, and accented when selected.
- An endpoint inside a deeper scope is drawn at the slot of the sub-patch card that holds it. A
  cable wholly inside a collapsed sub-patch is not drawn at the outer level.
- A cable that closes a loop is legal.

### 8.4.4 Adding a node

- The add menu opens by: a double-click on the canvas, `Tab`, a long press on the empty canvas
  (touch), and a click on a pill (a seeded menu).
- The menu holds a search field that keeps the focus, a rescan button, tabs and a list. Its
  position is clamped to the visible viewport, with the soft keyboard taken into account.
- Tabs: all; one per engine; custom (the private library and this patch); VST3 plugins. The menu
  opens on the tab the user picked last, or on the engine of the seed.
- A row shows the engine, the type name and the source (the bundle, `this patch`, `custom`,
  `plugin`, or `unavailable`). An unavailable type is dimmed and cannot be picked; its tooltip
  says what is missing.
- Ranking, best first: exact name; name prefix; start of a word in the name; substring of the
  name; tag; bundle; doc. Ties: earlier match, shorter name, alphabetical. An empty query shows
  the catalog order.
- Keys: Up and Down move (and wrap), Enter picks, Tab and Shift+Tab change the tab, Escape
  closes.
- A seeded menu shows where it came from, lists only the types with a compatible slot on the
  opposite side, and links the first such slot on creation. A seed from an input that takes one
  source replaces its cable; a seed from a list input adds to it.
- The boundary port types of a sub-patch are offered only inside a sub-patch.
- After a pick, a ghost card follows the pointer on the grid. A click on the canvas commits; a
  click outside or Escape cancels. On touch the finger holds the ghost and the lift commits.
- The add and its link are one undo step. The new node is selected.
- Rescan reports how many types were added, reloaded and removed.

### 8.4.5 Selection

- A selection is a set of nodes and a set of cables, per panel. It survives a tab switch. The
  load of another patch clears it.
- A click replaces the selection. A click with Shift, Ctrl or Meta toggles. A marquee selects by
  area. A click on the canvas clears.
- A multi-select mode for touch makes every tap a toggle.
- One selected node opens the side inspector of the panel.
- "Select for reference" is one app-wide pick (a variable or a node output) that a param row can
  take as its source.

### 8.4.6 Clipboard

- Copy gives a self-contained fragment: the nodes, their cables, their ports and their nested
  scopes. It goes to the system clipboard as text.
- Paste puts the fragment on fresh identities into the entered scope, centred on the viewport,
  and selects it. One undo step.
- Cut is copy and delete in one step. Duplicate is copy and paste at an offset of one grid step
  multiple.
- Paste works without a secure context.
- Delete removes the selected nodes and cables in one step.

### 8.4.7 Sub-patches

- Group makes a sub-patch from the selection, at its centroid. Expand dissolves a sub-patch into
  its nodes.
- A sub-patch is a normal card. Its slots are its boundary ports, labelled by the port names.
- Enter: double-click the card. Exit: the breadcrumb (any level), Escape (one level), or a zoom
  out far beyond the fit zoom. When the entered scope disappears, the editor climbs out.
- The scope path is navigation: it does not dirty the patch. Enter and exit fit the view.
- A duplicate of a sub-patch is an independent copy.

### 8.4.8 Camera

- Pan, wheel zoom and pinch. On touch, a double tap followed by a vertical drag zooms about the
  tap point.
- `F` and the fit button fit the scope. The load of a patch fits once. An interactive add never
  moves the camera.

### 8.4.9 Keys and menus

| Key | Action |
| --- | --- |
| Ctrl/Cmd+A | Select all in the scope |
| Ctrl/Cmd+C, X, V | Copy, cut, paste |
| Ctrl/Cmd+D | Duplicate |
| Ctrl/Cmd+G | Group into a sub-patch |
| Tab | Add menu at the pointer |
| Delete, Backspace | Delete the selection |
| F | Fit |
| Escape | Close the menu; else clear the selection; else exit one scope level; else the shell |

- Keys stand down in a text field, in an open dialog and in an inactive panel.
- The menu of an output pill (right click or long press): copy name (the expression that reads
  the output), select for reference.
- The overflow menu of the shell holds the editor commands: select all, clear selection,
  multi-select mode, delete, group, copy, cut, paste, duplicate.
- An errors indicator in the shell lists each node in error; a click selects the node.

### 8.4.10 Rules of the editor

- Every edit is an op. The browser holds a replica.
- One gesture is one undo step. The steps of a drag are previews without history.
- Each feature behind hover or right click has a touch door.
- A preview is drawn by the same component as the real thing.
- The geometry of the canvas is arithmetic from shared constants, not a measurement of the DOM.
- One change of the document wakes only the readers of the record it touched.
- One gesture recognizer defines tap, double tap, long press and drag slop for the whole app.
  One drag engine serves every drag. One overlay positioner serves every menu and popover. One
  list navigation serves every keyboard list.

## 8.5 The inspector

### 8.5.1 Hosts

- **Side pane:** an editor panel shows the inspector of its one selected node in a side pane: at
  the right edge when the panel is wide, as a bottom sheet when it is tall. The pane resizes by
  a drag, between 10 % and 90 % of the panel. A switch keeps it shut.
- **Inspector panel:** a panel bound to one node, chosen from a list or by a node dropped on it.

### 8.5.2 Content for a node

1. **Identity:** the name (click to rename), the type, a state badge with the tier the node runs
   on, "save to the private library", and "open plugin editor" for a type that has one.
2. **Docs:** the first line of the doc; the rest in a disclosure.
3. **Sub-patch:** the member count and Expand, in place of params.
4. **Tools:** a search over every group; a "non-default only" toggle with a count; "clear
   non-default" (the present state becomes the zero point of the node); a source filter
   (constants or expressions).
5. **Group tabs:** one per param group that has a shown param. The main group comes first.
6. **Param rows.**
7. **Slots:** the coupling of each input slot.
8. **Metadata:** an output slot select; the update rate; dropped frames per second; then shape,
   dtype and every meta entry of the newest frame.
9. **Error:** the message and a restart button.

### 8.5.3 Param faces

| Param | Face |
| --- | --- |
| Number | A slider over the soft range and a number field. The field is not clamped to the soft range. |
| Integer with options | Segmented buttons and a number field. |
| Vector | One number field per dimension. |
| Colour | A swatch that opens the colour picker. |
| Envelope | An ADSR bar that opens the envelope panel. |
| Boolean | A toggle. |
| String with options | A select, with a rescan button when the node can refresh its options. |
| String | A text field. |
| Pulse | A button. |
| Unknown | A JSON text field. |

- The face of a param never changes with its source. A driven param shows its live value,
  disabled.
- The label of a row opens it: the source switch, the expression editor, MIDI learn, and for a
  vector the typed form.
- A vector has two views: one source for all elements, or one row per element (`R G B A`,
  `A D S R`, `[i]`) with its own value, source, MIDI learn and error.
- A source error shows under its row at all times.

### 8.5.4 Groups, sections, lists, show rules

- A group can have sections. A line separates two shown sections.
- A list section has a count row with minus and plus, and one block per open slot with a grip to
  reorder, its number and a remove button. Reorder and remove are one undo step each.
- A list section with the colormap widget is drawn whole: a gradient bar with presets, a track
  of stops, and per stop its place, its colour and its blend to the next (`linear`, `smooth`,
  `step`).
- A param shows only while its show rules hold (part 04.3). A hidden param keeps its value and
  its source. A group tab with no shown param is absent.

### 8.5.5 Sources

- A param has exactly one active source: a constant or an expression. A switch to expression
  with no saved text seeds the present value as a literal. A switch back keeps the text.
- The expression editor is one line with Python highlighting. It commits on Enter or blur. A
  window under the field shows the doc of the function at the cursor and the completions.
- Completions cover everything an expression can read (part 03.4): nodes, their outputs and
  params, playheads, variables, time, the built-in generators with their arguments, and math.
- Other ways to set a source: drop a node or a variable on the row; the row menu (LFO, noise,
  reference the selection); MIDI learn.
- Row keys, on the row under the pointer or in focus: `c` constant, `e` expression, `r` reset,
  `x` random in range, `l` LFO, `n` noise. A visible hint names them.
- MIDI learn opens every port for a set time, waits for the first control that moves, and writes
  the reference to that variable. It ends on timeout, on a second click, or when the devices
  leave.
- The common params of a node (part 04.3) are ordinary rows in their own group.

## 8.6 The control panel

- A control panel shows one variable group as a surface of widgets. Tabs show every control
  group: add, rename, close. To close a tab removes the group and its variables in one undo
  step.
- A widget is a variable with a control record: kind, range, step, options and its cell.
- Kinds: knob, slider, number, text, toggle, dropdown, paint, vector, colour, envelope.
- The layout is a grid of 16 columns. Rows are near square with a touch floor. A cell snaps to
  the grid.
- **Edit mode:** a palette of widget kinds (tap places one; drag places it at a cell); drag a
  cell to move it; a corner handle resizes; delete removes; double-click the label renames. Each
  gesture is one op.
- A settings popover on a widget: name, kind (only kinds that fit the value), source (value or
  expression), range, options, MIDI learn, delete.
- **Binding:** drag a widget, or its label in play mode, onto a param row. The param source
  becomes a reference to the variable.
- A widget can be computed: from an expression, a dropped node or MIDI. A computed widget is
  dimmed and does not take input. A broken one is marked.
- **Play mode:** widgets are live. A drag previews without history; the release commits.
- Edit mode is part of the state of the panel.

## 8.7 The variables panel

- A variable is `group.element`. Every expression reads it as `vars.group.element`.
- The document holds the configuration of a variable. Values travel as frames, and only for
  groups that a client has open.
- Forms: number, text, list, image (a paint pad).
- Groups collapse. A group shows its origin: state machine, control panel, MIDI device, system.
- Locks: a configuration lock (name, widget, membership) and a value lock, on a group and on an
  entry. Device, machine and system groups are locked.
- A row has a grip that carries the reference for a drag, the name, the value, the form and a
  delete button. Buttons add an entry and add a group.
