# 08. goofi-pipe: the application

goofi-pipe is the real-time program. One binary serves the app and is its CLI. This part is in
three files: this file (8.1 the server, 8.2 the shell, 8.3 the workspace), `08-pipe-editor.md`
(8.4 to 8.7) and `08-pipe-panels.md` (8.8 to 8.15). Sections 8.1.2 and 8.1.3 hold for the server of every program
of the ecosystem.

## 8.1 The server

### 8.1.1 Flags

| Flag | Default | Effect |
| --- | --- | --- |
| `--port N` | 8000 | The port. |
| `--bind HOST` | 127.0.0.1 | The address. An address beyond the machine prints the warning of part 01.6. |
| `--extra-nodes ROOT` | — | A folder of node files, scanned after the shipped bundles. Repeatable. |
| `--load PATCH` | — | Open this patch at the start. |
| `--headless` | — | Serve the API alone. The routes of the app are not mounted and the layout ops leave the op table. |
| `--mcp` | — | Serve the MCP endpoint. Off by default. |
| `--demo` | — | Withhold what a public instance cannot offer (8.1.4). |
| `--debug` | — | Open the development routes: the gallery of UI primitives and the other development surfaces. |

- Each flag has an environment variable of the same meaning. A headless build leaves the app out
  of the binary and needs no Node.js. A boot-only run scans the node library and exits; CI uses
  it to prove a build.

### 8.1.2 Connections

| Connection | Content |
| --- | --- |
| Control (WebSocket, JSON) | Op calls with their replies; previews; events (8.1.3). |
| Data (WebSocket, binary, one per node output) | Frames in the wire format; the client declares what it can draw (part 02.10). Variable values use the same connection kind. |
| Presence (WebSocket) | The peers and their pointers. |
| Terminal (WebSocket, one per agent instance) | PTY bytes both ways; resize messages. |
| Exec (HTTP) | Command lines of the CLI, run as ops. |
| Patch file (HTTP) | Download and upload of a patch through the browser. |
| MCP (HTTP) | One tool that runs command lines of the CLI. |
| Plugin assets (HTTP) | The built frontend of each plugin. |

- A call carries the actor (one identity per browser tab, kept across a reload) and the
  navigation context of the client: the active tab and panel, the scope path of each editor and
  the selections. Undo and redo return the context of the step, and the client restores it.
- Live values of driven params, and source errors, arrive on the control connection for the
  nodes a client watches. Each fact has one path to the client.
- Every connection reconnects by one policy: a backoff from 250 ms to 5 s, and never after a
  close code that says the target does not exist.
- A new server instance is recognized by its identity: the client resets its replica, its
  history labels and its selection. The same instance after a short loss reseeds the replica.
- A client whose protocol version differs from the server shows one banner with a reload
  button and does nothing else.
- A call made while the connection is lost fails at once. Nothing is queued or sent again.

### 8.1.3 Events

The server pushes: the hello (the protocol version, the instance, demo, examples, history
labels); the document seed and its deltas; node stages, health and rates; evaluated param values
and source errors; log batches; the unsaved flag and the save path; the status of a recording;
the catalog of node types; the roster of agents; the history labels.

### 8.1.4 Demo mode

- Demo mode removes from the op table: directory browsing, agents, saving and loading a patch, the
  recovery ops, and saving to the private bundle. It does not mount the
  exec, MCP, terminal, patch-file and plugin routes. No audio engine runs and no plugin loads.
- In the app: a `demo` label; no save and load controls and no file shortcuts; no recovery
  question; the agent panel says that agents are off; the console prompt is off; an examples
  button and "reset to example" exist when the server lists an example set.
- Each example of a demo set is an instance of its own; a pick navigates to it.

### 8.1.5 Start and stop

- Start order: the session; the node library scan with the start screen; the engines; the patch
  (the one named at the start, else a new one); the server. The URL is printed.
- Stop has one order, executed by the session (part 02.3): takes are closed, agents are stopped,
  children end, engines stop, the session is released. A second interrupt forces the exit.

## 8.2 The shell

The shell is everything outside the panels: one top bar, the workspace, and a small set of
overlays. There is no menu bar and no status bar.

### 8.2.1 The top bar

- Left: the strip of workspace tabs.
- Always visible: the examples button (when the server lists examples); a `disconnected` badge;
  a peers badge with a count (when there is more than one client); an agents button with a count
  and one row per instance; an errors button with a count and one row per node in error (a
  click selects the node).
- A group that moves into an overflow menu when the bar is narrow, lowest priority first: plugin
  header entries; the paint rate; the recording indicator; the path chip; save as; load; save;
  new; redo; undo.
  - The paint rate is the paints per second of the viewers.
  - The recording indicator is a dot with the elapsed time of the take from the backend. It
    warns while a stream loses frames.
  - The path chip shows a mark when the patch is unsaved, and the file name or `untitled`. Its
    tooltip is the full path.
  - Undo and redo carry the label of the step in their tooltip and are disabled without one.
- The overflow menu is always present. It holds the moved items, then the commands of the
  active editor (part 08.4.9).
- An item stays whole or moves whole.
- The title of the document is the unsaved mark and the file name, or `goofi-pipe`.
- The `disconnected` badge shows only after an established connection is lost. The paint rate is
  hidden while nothing paints.
- A click on an agent row brings the panel of that agent to the front and asks the close
  question (8.12).
- The overflow button is accented while multi-select mode is on. With no active editor and
  exactly one editor panel, the editor commands act on that panel.

### 8.2.2 Files

- **New** makes an empty patch.
- **Open** and **Save as** use one file dialog: a column of roots with the recent folders; a
  path bar with an up button and an editable path; rows sorted by name, date or size; patch
  files marked. Save has a name field and "download a copy". Open has "upload".
- Save to an existing file asks before it overwrites. Save to a folder is an error.
- While a save or a load runs, the dialog shows the log rows the backend writes and cannot be
  closed. A failure keeps the log and shows the error.
- **Save** writes to the remembered file, or opens the dialog when there is none.
- The save path and the unsaved flag come only from the backend.
- **Recover:** once per server session, when recovery copies exist, a dialog lists them with
  their age. A row recovers one; a row can be discarded; all can be removed; skip is the
  default.
- **Examples** open from the examples button.
- In the dialog a click selects a patch file and, in a save, copies its name. A double click
  enters a folder or opens the file. Save as suggests the present name.

### 8.2.3 Undo and redo

- One history serves all clients (part 03.8). The client shows only the labels.
- An undo or a redo restores the navigation context of the step: the tab, the panel, the scope
  and the selection. The nodes it touched flash.
- A refused undo is a toast.
- A gesture made of several ops is one step. A batch is one step.

### 8.2.4 Presence

- Each client has a hue. The pointer of each peer shows as a dot, only for peers on the same
  workspace tab.
- The dot of a peer leaves when its pointer leaves the window or the window loses focus.

### 8.2.5 Overlays

| Overlay | Behaviour |
| --- | --- |
| Notification | One message at a time, the newest wins. It has a tone: information, warning or error. A click dismisses it. |
| Tooltip on touch | A long press on an element with a tooltip shows the tooltip. |
| Disconnected frame | A ring around the window while the connection is lost. The replica keeps its last state. |
| Drag ghost | A chip follows a drag outside its home. A target shows a drop hint. Escape cancels. |
| Quick pick | One transient list at a point, for a choice inside a gesture. |
| Confirm | A question with named answers. |

- A notification leaves by itself after a short time. Notifications report, among others: a
  failed save, recover or discard; a refused undo; a failure of a plugin; an unexpected exit of
  an agent; MIDI learn with no device.

### 8.2.6 Keys of the shell

| Keys | Action |
| --- | --- |
| Ctrl/Cmd+S | Save |
| Ctrl/Cmd+Shift+S | Save as |
| Ctrl/Cmd+O | Open |
| Ctrl/Cmd+Z | Undo |
| Ctrl/Cmd+Shift+Z, Ctrl/Cmd+Y | Redo |
| Escape | The innermost thing that is open closes first; the shell (panel maximize) is last. |

- Undo, redo and Escape stand down while a text field has the focus. Every key of the shell
  stands down while a dialog is open. There is one test for each of the two conditions.
- Save and Open act also while a text field has the focus, and always suppress the dialog of the
  browser.

## 8.3 The workspace

### 8.3.1 Model

- In this part "workspace" means the panel workspace of the app. It is the arrangement of the
  patch (part 03.7): tabs, and in each tab a tree of splits and panels. It is part of the patch:
  saved, shared by all clients, undoable. The folder of a patch is the patch workspace (part
  03.1).
- There are no floating panels and no separate windows.

### 8.3.2 Panel types

| Type | Content |
| --- | --- |
| Empty | A launcher grid of every other type. A split creates one. |
| Node editor | The canvas of the patch (8.4). The default of a new tab. |
| Inspector | The params, metadata and error of one node (8.5). |
| Viewer | The live frames of one output slot (8.8). |
| Control | Widgets over one variable group (8.6). |
| Variables | The variables of the patch (8.7). |
| State machine | One machine on a canvas with its score (8.9). |
| Recorder | Arm outputs, start and stop a take (8.10). |
| Console | The log and the command prompt (8.11). |
| Agent | A terminal on an agent harness (8.12). |
| Plugin panels | Panels that plugins register (8.13). |

- There is no limit on the number of panels of a type.
- A new control panel gets a new variable group. A new machine panel shows the first machine,
  or a new one when the patch has none. A panel of an unknown type says so.
- The panel types are one open registry: a built-in type and a plugin type register the same
  way.

### 8.3.3 Mechanics

- **Tabs:** click selects; a plus adds; a double click renames; a close control closes (not the
  last tab); a drag reorders.
- **Panel header:** a content button (icon, title; opens the list of types), split right, split
  down, maximize, close. Controls that do not fit move into an overflow menu. A right click or a
  long press opens the same commands as a menu.
- **Move and dock:** drag a header onto another panel; the nearest edge half lights; the drop
  docks it there. Drag a tab onto a panel to move its whole tree. Drag a panel onto the tab
  strip to make a tab. Every move works with a mouse, a pen and a finger.
- **Corner grips** (fine pointer): a drag inward splits; a drag onto a sibling joins.
- **Splitter:** a drag previews and the release commits once. A panel keeps a minimum share and
  a minimum size.
- **Maximize** is per client and is not saved. Escape ends it. A panel is not remounted when it
  is maximized.
- **Active panel:** a press makes a panel active. The active panel is outlined and takes the
  panel keys. There is one notion of "active".
- **A panel over one of several things** is one pattern with one bar: a panel bound to a node
  (inspector, viewer, console filter) picks it from a list or takes a dropped node and shows the
  health of the node; a panel over instances (machines, control groups, agents) shows them as
  tabs with rename, close and add.
- **A side pane** inside a panel (the inspector in an editor, the score in a machine panel)
  takes its space from the canvas, at the right when the panel is wide and at the bottom when it
  is tall, with a seam that resizes it.
- A new tab is named `Tab N`. To close the front tab selects its neighbour. A new or moved panel
  becomes active. The close control is disabled on the last panel of a tab.
- Keys of the tab strip: Left, Right, Home and End select a tab. A rename commits on Enter or
  blur; Escape cancels it.
- The node list of a bound panel has an empty choice that unbinds. A new binding resets the
  slot.

### 8.3.4 Where state lives

| State | Home |
| --- | --- |
| Tabs, splits, shares, panel types, panel state | The patch. Undoable. Dirties the patch. |
| Viewpoint: the active tab and panel, the scope path and camera of each editor, the size of each side pane, the filters and prompt history of a console | Per client; the patch file keeps the viewpoint of the client that saved it; never shared; never dirties the patch. |
| Maximize, selection, multi-select mode, the reference pick, which agent a panel shows | The page. |
| The sort order of the file dialog | The browser. |

- Navigation never dirties the patch: selecting a tab, entering a sub-patch, focusing a panel,
  maximizing.
- A long-lived resource of a panel (a terminal, a stream) lives outside the panel, so a closed
  and reopened panel loses nothing.

### 8.3.5 Devices

- One interface serves touch, tablet and desktop in both orientations.
- There is no device mode. One condition (no hover and a coarse pointer) enlarges the hit size,
  the splitter and the headers, keeps hover-only controls visible, and enables the long-press
  doors.
- Modality comes from the event, not from the device: a pen on a touch screen behaves as a
  pointer where it can.
- A panel sizes by container queries against its own box, never against the window.
  Orientation is asked of the panel.
- Overlays respect the safe areas and the soft keyboard. Motion respects the reduced-motion
  setting.
