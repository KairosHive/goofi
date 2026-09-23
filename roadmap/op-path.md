# The op path: ops that do not block, and edits under continuous motion

Two findings of the frontend performance audit (`frontend-performance.md`) that share one design.
Every op is the standard interaction — UI, CLI, MCP, scripts and tests all speak it — so the rules
for what an op may hold, what it defers and how a gesture feeds it must be decided once, for all
of them, and not in the control that happens to emit the most.

## What is known

- An op holds the graph lock for its whole run. In the debug build that was 9–65 ms per op; the
  write path since moved its diff, version and encode off the lock, and only the projection
  stays under it.
- A knob, a slider or a number field under a pointer emits one op per pointer event. Each op is
  a full round trip with its own undo entry; the document replica now wakes only the readers of
  the leaves a patch names, so the client cost per op is small, but the socket's event drain
  waits on the reply of every one of them.
- The live value in a control (`liveValue.svelte.ts`) shows a committed value until the source
  echoes it, so a gesture reads as continuous whatever the backend's pace.

## Decisions already taken

- A per-socket op queue that held an op's events behind its reply was built and TAKEN OUT: a
  queue beside the op path is a second scheduler, and a slow op still parked everything behind it.
- A rate limiter inside `useLiveValue` (newest value wins, one send per 40 ms, the next send
  waiting on the reply) was built and TAKEN OUT: the edit path for a gesture — what is sent, when,
  and what the control shows meanwhile — is one design across every control, and it depends on
  what an op is allowed to cost. It is made after the op path, not before it.

## Open

- **Ops that do not block.** Only a write should hold the graph; every op must stay atomic; a
  slow op must not park a socket's event drain. To be designed whole, with the op table: which
  ops read, which write, and which run off the lock entirely.
- **Continuous-motion edits.** Whether a gesture is one op with a stream of values inside it, a
  coalesced sequence of value ops with one undo entry, or something else; and what the control
  shows between a send and its echo. Same answer for knob, slider, number field and the control
  panel's widgets.
