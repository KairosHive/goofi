# Frame copies across the native node boundary

## Remaining

- Hand the node its input frame as the bytes the wire already carries (the GOOF frame off
  shared memory), not decoded and re-encoded by the host.
- Let a node's output that is one of its inputs unchanged cross back as a reference to it.
- Encode a published frame straight into the iceoryx2 loan, sized by the frame's known length.
