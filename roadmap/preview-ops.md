# Preview ops: a drag is one op, not a burst of them

A knob turn sends a `node param edit` per pointermove today. Each one takes the graph lock,
pushes a history entry, projects and broadcasts the doc, and replies; the control socket reads the
next message only after the reply, so a burst waits in the socket buffer where nothing can fold it.
The layout splitter avoids this with a local override in panelty and one `layout split edit` on
pointer-up. Both become one design: the op, sent with a `preview` flag, run through the same arm
and the same command, and treated differently only by the history.

## Decisions

- **The flag lives on the op envelope.** `{ id, op, payload, actor, preview: true }`. The arm and
  the command are the ones the commit uses; a preview is never a second write path. The browser is
  the only client that sends it. Scripts and the MCP may, and a value they preview and never commit
  reverts when their socket closes.
- **Every undoable command names its key**: node and param for a param edit, the split id for a
  split edit, the panel id for a panel edit. The history derives the merge from it; the client
  sends no token.
- **The history keeps open previews per actor, keyed by the command.** The first preview on a key
  stores the inverse against the last committed state; later previews on it store nothing. The
  commit, the same op without the flag, executes and pushes one entry whose inverse is that stored
  baseline, then clears the key. One undo returns to the state before the drag. No preview is ever
  an undo entry.
- **Socket close reverts the actor's open previews** to their baselines. Undo and redo by an actor
  with open previews revert those first, then flip.
- **A preview changes the graph for real**: the engine sees it through the same path as a commit
  and the doc patch reaches every client, so a second browser sees the knob move. It does not push
  history and does not set the dirty flag; the commit does both.
- **Latest wins on both ends.** The control handler keeps reading the socket while an op runs and
  holds previews in one slot per key, dispatching the newest after the op in flight; ordinary ops
  keep their strict order. The client sends at most one preview per key per animation frame. This
  is not the per-socket op queue `backend-architecture.md` rules out: nothing is held behind a
  reply.
- **panelty's local resize override and its `_sent` baseline go.** The splitter previews like
  everything else; the preview's doc patch is what the splitter draws.

## Remaining

1. The data worker renders from the latest `settings` and `place` per animation frame, the way
   `paint` coalesces, instead of a synchronous render per message; a resize with several viewers
   replays every intermediate size today. Hold the 32-px spec renegotiation in `ViewerFeed.svelte`
   until the size has been still for a frame or two. Independent of the rest.
2. `key()` on the commands; the preview map in `CommandHistory`; the flag in the envelope and in
   `ControlClient.call`; the revert on socket close and before a flip.
3. The latest-wins slot in the `/control` handler.
4. `useLiveValue`: `input` previews, `commit` commits; Knob, Slider and NumberInput follow. Panelty
   resize through `resizeSplit` with the flag, and the override deleted upstream.
5. A situation: a drag of N previews and one commit is one undo entry and one dirty flip; a socket
   that closes mid-drag leaves the committed value; a second browser sees the preview.

## Open

- Whether the tail should project touched paths only (§3.12) as part of this: a preview per frame
  projects the whole doc until it lands.
