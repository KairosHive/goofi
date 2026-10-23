# The data layer: remaining work

The transport (`goofi_transport`: hub, slots, doors, queues, the exchange) and the one timing
module (`goofi_supervisor::timing`) shipped on 2026-10-22; AGENTS.md describes them.

## Remaining

- Windows and macOS: the exchange's sections (`CreateFileMappingW`, `shm_open` under the
  31-character name limit) and the `select` wait compile against the bindings but have not run.
  Run the children, python, nodes and recording situations there.
- The measurements `performance-audit.md` lists under "Measure again".

## Not to be done

- A generic message bus, a schema or a serialisation layer for the in-process plane: a
  `Payload` is an `Arc` of what the node made.
- Cross-process publish/subscribe. Nothing needs it; a child answers calls.
- Priority inheritance or a wait-free cell: no real-time thread waits on the transport.
