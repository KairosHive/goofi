# The data layer: goofi's own transport, and one timing module

Decided 2026-10-22, from the performance audit (`performance-audit.md`).

## Decisions

- **iceoryx2 goes.** Every publish/subscribe path goofi has is in-process: a node's output slots,
  the doors nodes park on, the recorder's queues, the view doors and the variables. Only the
  exchange with a child, a hosted Rust node or a Python subprocess, crosses a process. Paying
  shared memory, a service directory and a datagram socket per wake for an in-process `Arc` is
  what the audit's hop and wake costs were. What remains of iceoryx2's weight with it: the
  Windows handle table, the compile-time `C:\Temp` root, the node directories a crash leaves, the
  per-service limits fixed at creation, and the `SO_RCVTIMEO` wait that rounds every deadline to
  a kernel tick.
- **One transport crate, two channel kinds, one name space.** `goofi-transport` keeps its names
  (`names.rs` is unchanged: a service name is still a pure derivation from the graph, and still
  the rendezvous) and gains a `Hub`: the per-session registry that hands any end its slot, door
  or queue by name, whichever end comes first. In-process channels carry a `Payload`: a `Data`
  as the `Arc` it already is, or a GOOF frame already encoded (`Wire`), the one case a producer
  makes bytes on purpose, the 8-bit texture tap. A frame is never encoded to cross a wire.
- **Latest wins by construction.** A slot is one cell: a sequence number and the newest payload.
  A subscriber remembers the sequence it last took; `receive` hands the newest payload once, or
  nothing. There is no queue to overflow and nothing to drop. A `held` slot (the variables) hands
  its newest payload to a subscriber that comes late; a node's slot does not, so a link never
  replays. The recorder's channel is the one FIFO: a bounded queue per arming, oldest dropped,
  the index gap being the count.
- **A door is a `timing::Event`.** 256 ids in a bitset under a mutex, a condvar to park on. A
  ring sets a bit and wakes; a wait takes every bit set, so ids coalesce and each wakes once. The
  deadlines are the kernel's high-resolution ones (`futex` and `pthread_cond_timedwait` with a
  timespec; Windows under the 1 ms timer the supervisor already asks for), never a socket timeout.
- **Lock-free is not bought where it buys nothing.** A cell's mutex is held for a pointer swap;
  no real-time thread touches a transport object (the audio callback reads its own rings). What
  the audit charged to iceoryx2 was syscalls and copies, not contention.
- **The exchange is shared memory plus a doorbell.** Payloads cross in a mailbox per direction:
  a seqlock header, the newest message, in a segment named by the session and the exchange's
  number; a message larger than the segment moves the mailbox to a bigger segment, named by its
  generation, and the reader follows. Segments are `shm_open` on Unix (31-character names, for
  macOS) and pagefile-backed sections on Windows (`Local\` namespace); the parent unlinks what it
  made, the session sweep removes what a crash left under the session's prefix. The doorbell is
  a loopback TCP connection the child makes to a port the parent listens on, with a token in the
  environment; one byte per ring, waited on with `poll` (or `select`), never a socket timeout.
  Neither side polls: a wait ends on a ring, a deadline, or the other side's death.
- **One timing module**, `goofi_supervisor::timing`: `Event`, `Pace` (a periodic grid: the next
  deadline, and a duty that overran restarts from now), `sleep_until`, and the readable-wait on a
  socket the exchange parks on. No other code sleeps in a loop, sets a socket timeout or
  computes a grid of its own.

## Interface

- `Hub::new(session_id)`; `hub.publisher(name, held)`, `hub.subscribe(name)` (the subscriber
  count is the slot's demand), `hub.door(name)` (a `Door`: `ring(id)`, `wait`, `wait_until`),
  `hub.record_port(name, depth, bell)` and `hub.record_subscriber(name, depth)`.
- `Payload::{Data, Wire}`; `Out::{Wire(&Data), Bytes(Bytes), Frame(&Data), Record(&Data)}` on
  the runtime, so an executor says what goes out and the runtime never encodes for a wire.
- `Exchange::open(&hub)` → the environment the child needs; `Served::open_from_env()`; the same
  `[u32 seq][frame]` protocol, `ask`, `answer`, `wait`.
- The recorder takes `Data`: the drain hands the frame it received, the lanes queue it, and the
  file reads its samples through the array's own bytes.

## Remaining

- Measure again with the audit's probes on the new plane: the hop, the cap table, the audio
  tick, the graphics tap, the viewer pace. The targets: a busy hop under 5 µs, a 1 kHz cap
  delivering 1 kHz, a 60 fps viewer cap delivering 60.
- Windows and macOS: the exchange's sections (`CreateFileMappingW`, `shm_open` under the
  31-character name limit) and the `select` wait compile against the bindings but have not run.
  Run the children, python and nodes situations there.
- Then the audit's next findings on the data path: `Buffer` on a ring, the Python marshalling
  copies, the per-op document projection.

## Not to be done

- A generic message bus, a schema or a serialisation layer for the in-process plane: a
  `Payload` is an `Arc` of what the node made.
- Cross-process publish/subscribe. Nothing needs it; a child answers calls.
- Priority inheritance or a wait-free cell: no real-time thread waits on the transport.
