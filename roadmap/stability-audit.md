# Stability audit, 2026-10-10: remaining fixes

Found by reading; each fix lands with a fixture that fails on the broken variant first.

## Panics and hangs under the graph lock

- A bare generated-variable expression (`__v0`, `__v3[1]`) passes the rewriter with no refs and
  `derive` indexes `refs[0]` (`goofi-graph/src/lib.rs:2681`). A variable expression repeats the
  panic on every settle. Refuse `__v` identifiers at the rewriter, or make the bare check demand a ref.
- `pack_right` scans rows `y0..y0 + 400` but `clear_of` refuses `y < 0`; a node moved far above the
  origin followed by an add with no `pos` loops for ever (`goofi-graph/src/canvas.rs:70-80`).
  Scan from `max(y0, 0)` and bound the columns.
- A hand-edited document can install a scope cycle or a non-facade parent; `ancestors` and
  `member_in` have no cycle guard, so group and arrange never return. `admit` must check scopes.
- A `000000000000` record key restores a node at `Uid::VARIABLES`; refuse the key.
- A machine `after` interval too small to move f64 time spins the machines thread; a non-finite
  duration prints `inf` into Python source. `Machine::check` must require a finite, representable step.
- `free_cell` spins on a control box with a huge or NaN height.

## Deadlock and ordering

- `compile` takes `by_source` then `codes`; `release` takes `codes` then `by_source`
  (`goofi-python/src/inproc/expr.rs:181-221`). One order, and no Python attach under either lock.
- A preview in flight when its `/control` socket closes stays applied, with no history entry and no
  dirty flag: `end_previews` is not ordered after the pending dispatch (`goofi-bridge/src/lib.rs:1297`).
  Await `pending` first. Also: an unparsable request gets no reply; the per-socket queue is unbounded.
- `Recorder::settle` waits with no deadline; a drain whose subscriber failed to open never reports,
  and `record stop` and shutdown hang (`goofi-record/src/lib.rs:279`). `Ffmpeg::finish` likewise.
- `load_patch` stops a running recording before the parse can refuse (`ops/session.rs:512`).
- Host reads (`dir list`, `session status`, `midi list`, `log list`) run as read ops under the graph
  and history locks. Serve them off the lock, as `fsbrowse.rs` already claims.

## Engines

- A playing audio file settles the whole graph every control tick: each chunk is under a sixteenth
  of the playback ring, `room` reports a resize, and the playback ring's size cell is not in the
  instance's `wanted` list, so `fit_rings` never clears it (`goofi-audio/src/control.rs:529`).
- `Msg::Rings` swaps only the ring; voice and phasor buffers keep the seed capacity and the first
  large frame reallocates on the audio callback (`goofi-audio/src/runtime.rs:306-373`). Smaller
  callback allocations: VST3 `Changes::moved`, `midi_in` voices, `Inbox::last`, the RTKit claim.
- `Ui::run` turns a window-thread panic into a caller panic, also inside `Plugin::drop`; VST3 open
  and retune run on the window thread under the runtime lock and the graph lock.
- A failed GPU readback map leaves the slot unready at the ring's front for ever; nothing is reported.
- A computed param wider than `BLOCKS_RING` is dropped with no error on `/control`.

## Supervisor and tools

- Boot removes every other version's runtime tree with no liveness check
  (`goofi-supervisor/src/session.rs:85`); a dev build and a release share the root.
- `goofi-init` sweeps `target/` by mtime without cargo's build lock.
- The dist tool download and the `cargo --version` probe have no timeout.
- `WORKING` encoder cache is never invalidated and stores a candidate index.

## Frontend

- A refused delete leaves the canvas without the nodes while the document keeps them: xyflow removes
  them before `ondelete` and the catch never re-derives (`NodeEditorPanel.svelte:794`).
- The data worker treats every 4000+ close as permanent, but the backend sends 4004 whenever a slot
  does not resolve at connect time, which a reload does (`dataWorker.ts:89`). Reconnect, or have the
  registry rebind.
- Rejected param and control edits display as accepted: `liveValue` resets only when the source
  moves, and the inspector and control panel swallow refusals. Reset `pending` on rejection and notify.
- Duplicate and paste reject unhandled; add, group and auto-link log to the console only.
- The runtime overlay stash grows for nodes that left the document; agent kill has no catch; the
  params socket retries every 500 ms with no backoff; `JSON.parse` on inbound frames is unguarded in
  `paramLive`, `presence` and `termSession`; a replica's gap recovery closes the control socket and
  reports ops the manager executed as failed.
- ColorPicker keeps `dragging` after a strip press with no change; a cancelled touch cable leaves the
  window listener and highlight tags; a side pane that closes unpainted keeps a stale inspector and
  its sockets; Ctrl+F refits the canvas; touch placement commits on the first lift under a pinch;
  machine multi-delete is N history steps.

## Not read

`goofi-bridge/src/plugins.rs:830-879`, frontend tests, `generated.ts`, `vocab.ts`, Svelte styles.
