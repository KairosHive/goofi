# 09. The shared UI library

One UI library serves every frontend of the ecosystem. It holds only what does not know an
application: tokens, primitives, gestures and drags, overlays, the panel mechanics, plotting, the
data viewers, the op client and the browser end of the transport. The patch editor and the node
inspector belong to goofi-pipe. The dashboard belongs to goofi-data.

## 9.1 Tokens and theme

- Every style value comes from a token. A component holds no literal colour, size, radius,
  shadow, duration or layer number.
- Token families: surfaces and borders; text; accent; success, warning, danger and information
  with their fills; the colour of each data kind; the colour of each engine; syntax colours;
  spacing; type sizes; durations and easing; radii; shadows; layers; focus; safe areas; the hit
  size; the node metrics of the canvas.
- One dark theme. A second theme is a second set of token values and nothing else.
- Fonts: one for the interface and one monospaced for data, both shipped with the app.
- Icons: one vendored set drawn by one renderer. No text glyph stands in for an icon.
- One focus ring. Contrast is tested.
- The node metrics and the grid are generated from the backend constants (part 03.8).

## 9.2 The edit model

- Every control follows one model: input previews, change commits. A control shows its own
  value until its source echoes. Focus and blur without a change are not an edit.
- A primitive does not import application state. It takes values and reports changes.
- Every text field goes through one text input that sets the keyboard of the device: prose,
  identifier (no capitals, no correction) or path.

## 9.3 Primitives

| Primitive | Contract |
| --- | --- |
| Knob | A vertical drag over a fixed distance covers the range; a modifier makes it fine; a double click resets; arrow keys step; a click on the value edits it as text. |
| Slider | A range with its ends printed. |
| Number input | Enter or blur commits; Up and Down step; it clamps only when bounds are given. |
| Text input | Blur commits; Enter commits one line; a multi-line field keeps Enter. |
| Toggle | A switch. |
| Button | A momentary control: one press is one event. It can flash. |
| Select | A list of options with labels; an optional rescan control with a busy state; a stale value stays selectable. |
| Combobox | Filters by typing; only a listed name commits. |
| Segmented | An exclusive strip; also multi-select; icons, counts, a danger tone. |
| Choice grid | Tiles of an icon over a label. |
| Color picker | A swatch that opens a panel: a saturation and value square, hue and alpha strips, an eyedropper, hex and RGBA fields. |
| Curve picker | A glyph and a name that open a panel: the presets (step, linear, in, out, in-out, smooth) and a custom bezier with two handles. |
| Envelope picker | A shape that opens a panel: four knobs with fields, and an optional hold. |
| Colormap editor | A gradient bar with presets, stops, and per stop its place, colour and blend. |
| Paint pad | Colour, size, softness, erase, clear. One stroke is one op. |
| Popover | Anchored, clamped to the viewport, closes on an outside press or Escape; only the topmost reacts. |
| Menu | A list at a point or under a button, with optional search. |
| Quick pick | A transient list for a choice inside a gesture. |
| Dialog, confirm | Modal; the owner holds the open state; Escape or a press on the backdrop asks the owner to close. |
| Field | A label tied to its control; it can be a disclosure. |
| Others | Disclosure, bar, badge, chip, status dot, empty state, scroll area, busy indicator, tooltip, notification. |
| Terminal | A terminal view on a byte stream. |
| Code line | One line of code with highlighting, completion and a doc window. |
| File browser | Roots, a path bar, sortable rows. |

- The widgets of a colour, an envelope and a colormap are the same component in the inspector,
  on a control panel and in the score of a machine.

## 9.4 Gestures, drags, overlays, lists

- **One gesture recognizer** defines a tap, a double tap, a long press and the slop of a drag,
  with one set of times and distances per pointer kind.
- **One drag engine** serves every drag: nodes, values, widgets, panels, tabs, splitters, cells.
  A source offers tags; a target takes tags; inside the home of the source a drag is a move,
  outside it is a reference. A ghost follows the pointer; a target shows a hint; Escape cancels.
  Every drag works with a mouse, a pen and a finger.
- **One overlay positioner** places every menu, popover, completion window and tooltip, respects
  the soft keyboard and the safe areas, and portals above the panels.
- **One list navigation** serves every keyboard list: steps wrap; Home and End; the lit row
  scrolls into view.
- **One key rule:** a key belongs to a focused text editor first, to an open dialog second, to
  the active panel third, to the shell last. A handler that consumes Escape says so.
- During a drag, a disabled control does not block a drop on its row.

## 9.5 Panel mechanics, plotting, viewers

- **Panel mechanics:** tabs, a tree of splits and panels, headers with overflow, docking by
  drag, corner grips, splitters, maximize, a registry of panel types, and the two patterns of
  part 08.3.3. The mechanics know no application concept: a panel is an identity, a type and an
  opaque state. The layout itself is held by the application (in goofi-pipe: by the patch).
- **Plotting:** one renderer draws lines, images, fields and paths on a shared surface, off the
  main thread, with text drawn by the same surface.
- **Viewers:** the viewer kinds of part 08.8, their settings menu, their hover readout and the
  feed that subscribes a viewer to a stream. A frontend adds a viewer kind through a registry.
- **Dataset views** of goofi-data build on the same viewers: a sample is shown by the viewer of
  its kind.
- **Shared views:** the console (log and prompt) and the agent terminal are used by every
  program.

## 9.6 The client

- **The op client:** calls with replies; previews (the newest per key per frame); batches;
  events; the history labels; one reconnect policy (part 08.1.2).
- **The replica:** a read-only copy of a document, seeded whole and updated by versioned path
  operations; a stale delta is skipped; a gap forces a new seed. One change wakes only the
  readers of the leaf it moved.
- **The data client:** one connection per stream, shared by all readers; the declaration of what
  each reader can draw; stats of arrivals and drops.
- The types of the ops, the events, the document and the wire constants are generated from the
  backend.

## 9.7 Development surfaces

- A gallery shows every primitive in every state. It is served only in debug mode.
- A test surface in the page gives read access to the replica and to the stream stats. Tests
  drive the app through the ops and through real gestures; there is no second command
  vocabulary.

## 9.8 Open decisions

1. **The home of the graph canvas.** An earlier decision keeps the patch editor and the node
   inspector in the goofi-pipe frontend. A later decision makes every recipe a goofi graph and
   the recipe view the patch canvas with queue state on its edges (part 10.4.1). The two do not
   agree: goofi-data then needs the canvas and the inspector. Decide whether both move into
   the shared library.
2. A light theme.
3. A command palette over the op registry (the console prompt is the present command surface).
