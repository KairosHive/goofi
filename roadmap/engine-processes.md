# Engines as processes under one manager

Decided 2026-09-15 with the user: after the hosted native tier, each engine runs as a child of
the manager, joining its session. The manager keeps the graph, the document, the recorder's
manifest and HTTP; an engine child keeps its node instances, its clock and its own library.

## What already crosses a process boundary

- Every cross-engine frame rides iceoryx2 under names the resolver derives from the view, so no
  data path moves. A recording's frames reach the recorder over record services the same way.
- A child joins the session through `GOOFI_SESSION`, is listed by `session status`, dies with
  the manager, and logs through the child module.
- A hosted native node (`goofi host`) and the Python subprocess tier speak `goofi_transport::
  Exchange`, the request/response pair an engine proxy uses too.

## The seam, as a wire

`goofi_node::Engine` is one trait of some twenty methods. The manager registers an `EngineProxy`
per engine child; it implements the trait by sending each call over the child's control exchange
and reads health off a status stream the child publishes:

- `insert`, `remove`, `refresh_param`, `pulse_param`, `editor`, `view_demand`, `set_workspace`,
  `persist`, `shutdown`: one request each, the reply the method's answer.
- `settle(view, touched)`: the view serialized as plain data — edges, and per node its engine,
  name, generation, rings, TYPE NAME, params and binding views. The child holds the manifests;
  the proxy holds leaked copies rebuilt from the child's `describe` answers, which is what
  `library`, `universal_decls` and `normalize_params` answer from on the manager side.
- `scan`, `scan_own`, `remove_type`, `library`: requests whose answers carry the probe schema per
  type, leaked into manifests once per (type, stamp).
- `drain`: the child publishes `(uid, Status)` on a message service; the proxy's drain reads it.
  The child rings the manager's drain doorbell instead of notifying a shared `DrainWaker`.
- `published`, `take_edits`, `dirty`, `has_editor`: requests, answered from the child's state.
- `set_evaluator`: the signal child embeds its own interpreter for `nd()` bindings; the manager
  keeps its own for the graph's expressions. Two processes, two interpreters, no sharing.
- The patch clock: the manager owns `Time`; a child gets the origin at start and on every
  `reset_clock`, and derives `now` from it.

The concrete surface past the trait — the audio engine's `status`, `recording_boundary` and
`flush_recording`, the graphics engine's recorder hookup, `set_python`, `set_host`, `set_vst3`
and `set_ui` — becomes a request per call on the same exchange, typed per engine.

## Ownership that moves

- The window loop moves into the graphics child, and a VST3 editor into the audio child; the
  manager has no window. `goofi_window::Ui` is an engine child's.
- The boot scan runs in each child; the manager's `boot_done` is a request. A hosted native
  node is the signal child's grandchild.
- An engine that dies is reported on every node it held, and restarted by replaying the
  document's inserts and one settle; its nodes come back with their last-persisted state.

## Order

1. `EngineProxy`/`EngineServer` for the signal engine, behind `--engines processes`; in-process
   stays the default until the suite runs green both ways.
2. The graphics engine with the window loop.
3. The audio engine with its device clock and editors.
4. Crash reporting and restart.
