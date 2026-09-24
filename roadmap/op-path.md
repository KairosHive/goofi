# The op path: ops that do not block, and edits under continuous motion

Two findings of the frontend performance audit (`frontend-performance.md`) that share one design.
Every op is the standard interaction — UI, CLI, MCP, scripts and tests all speak it — so the rules
for what an op may hold, what it defers and how a gesture feeds it must be decided once, for all
of them, and not in the control that happens to emit the most. `backend-architecture.md` §3 owns
the transaction shape (`Txn`, one commit tail, `spawn_blocking`); this file owns which ops read,
which write, which run off the lock, and the gesture design.

## What is known

- An op holds the graph lock for its whole run; only the projection stays under it on the write
  path. A knob, a slider or a number field under a pointer emits one op per pointer event, each a
  full round trip with its own undo entry, and the socket's event drain waits on the reply of
  every one of them.

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
  shows between a send and its echo (`liveValue.svelte.ts` shows a committed value until the
  source echoes it). Same answer for knob, slider, number field and the control panel's widgets.
