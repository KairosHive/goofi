# Presence: who is in the patch, and where their pointer is

## Decisions

- **Its own socket, `/presence`, browser only.** Not `/control`: scripts, the CLI and the agent
  speak it too, and the peer count is "how many browsers". Not `/data`: that is the frame plane,
  binary and owned by the data worker. A cursor is not an op and never passes through dispatch.
- **The roster is the set of open sockets.** Open is join, close is leave; no heartbeat, no
  expiry, nothing under the graph lock. The server assigns each socket a color on join and sends
  the roster to everyone on every join and leave.
- **A pointer is `{ tab, x, y }`** with `x` and `y` as fractions of the window, sent at most once
  per animation frame, and a `leave` when the pointer exits the window. The arrangement is
  fractions of the window, so the point lands in the same panel on any screen. A peer on another
  tab is not drawn. Colors only, no names.
- **The overlay spans the whole window**, above the panels, and never dirties the patch.
- **The header shows the peer count** next to the session controls.

## Not to be done

- A graph coordinate for the canvas in the first version. Each client has its own viewpoint, so a
  window fraction is wrong there; add `{ graph: [x, y] }` when the canvas overlay needs it.

## Remaining

1. The `/presence` route with Origin/Host checks, the roster and the fan-out.
2. A `presence` store on the client: one socket per tab, the pointer sender, the roster.
3. The overlay and the header count.
4. A Playwright session with two pages: the count reads two, a pointer on one page draws on the
   other, and a tab switch hides it.
