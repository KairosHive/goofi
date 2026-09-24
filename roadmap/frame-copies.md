# Frame copies across the native node boundary

A big frame into a native signal node is copied about eight times per process call: decoded off
shared memory, encoded for the node ABI, decoded in the node, encoded back, collected, decoded
by the host, encoded to publish, and copied into the loan. A 512x2048 RGBA float frame (16 MB)
holds GraphicsIn at 18 Hz in a debug build, bound by those copies.

Remaining work:

- Hand the node its input frame as the bytes the wire already carries (the GOOF frame off
  shared memory), not decoded and re-encoded by the host.
- Let a node's output that is one of its inputs unchanged cross back as a reference to it.
- Encode a published frame straight into the iceoryx2 loan, sized by the frame's known length.
