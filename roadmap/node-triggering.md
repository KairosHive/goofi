# Node triggering modes

A signal node runs today on `autotrigger`, on a frame at a triggering input, or on the
`common.trigger` pulse (`goofi_runtime::common`). Which input triggers it is fixed by the node.
goofi-pipe needs a deterministic mode, where no packet is ever dropped and every pipe between
nodes is a queue, and goofi-data reuses the same node graph for its recipes: a graph whose nodes
are transforms and whose pipes are queues, scheduled by one node evoking the next in parallel as
packets arrive (`goofi-data.md`, ecosystem ground). Both need the node to say, per input slot,
what arrival triggers it. Status: planning, decided 2026-11-13. `triggers.md` is the separate
item for events delivered once to each listener.

## Decisions

- **A triggering mode interface per node.** A node is set up with a triggering mode, selected
  between different modes.
- **A setting per input slot.** The node gets a new vertical bar on its left side that exposes a
  setting per input slot: whether the node triggers on this slot's packet arrival, whether it
  waits until multiple slots hold a fresh packet (coupled input slots), and so on.
- **One modification to the node UI, one tweak to the backend.** The bar is a goofi-pipe UI
  modification to the nodes; the backend's node triggering logic takes a small tweak.
- **Queued and unqueued.** The transport supports both modes. goofi-pipe runs unqueued (latest
  wins, as today); goofi-data runs the same graph queued (every packet kept, in order).

## Remaining work

1. The triggering modes in the backend: per-slot arrival, coupled slots, and their interaction
   with `autotrigger` and the pulse; the SDK form for a node's default per slot.
2. The left-side bar on the node, one setting per input slot, through the op vocabulary.
3. The queued mode of the pipes, for the deterministic run of a patch.

## Open

- The right default for input slot coupling.
- Whether a mode is set per slot only, or a node-level mode constrains the slot settings.
