# Frontend performance: the audit and the order of fixes

Audit of 2026-09-16, run against the owner's report that the browser's frame rate falls as a
patch grows. Six static reviews (data plane, document sync, Svelte reactivity, control plane,
DOM/render, backend CPU) and two live measurements (a synthetic sweep at 0–60 nodes, and the
owner's own `test-patches/*.gfi`) produced 59 findings; merged to 44, each then checked by two
independent verifiers reading the code. 41 stand, 3 were refuted (listed at the end so nobody
looks again). The two structural answers have entries of their own: `viewer-render-surface.md`
and `data-plane-bandwidth.md`. This file is everything that can be done in the current design,
in the order it should be done.

Caveats on the numbers: headless Chromium with software GL, no audio or EEG hardware, no native
graphics window competing for the GPU, and a DEBUG backend (`cargo run`), on a machine shared
with other work. Absolute frame rates are optimistic; the scaling laws and the profile shares are
what to trust.

## What was measured

- Idle frame rate did NOT fall with node count at DPR 1: 57.8–60 fps from 0 to 60 synthetic nodes
  and on every real patch (music 57 nodes, thought-sphere 35, audio 23). Zero long tasks anywhere
  after the connect-time `hello` (850 KB, one 86–146 ms task).
- Main-thread busy share grows linearly with VISIBLE viewers: 1.5 % (0) → 6.7 % (5) → 16–18 %
  (30) → 37–39 % (60 visible). Per visible uPlot viewer ≈ 0.35–0.4 % at DPR 1; per redraw
  ≈ 0.15 ms at DPR 1 and ≈ 0.8 ms at DPR 2 (0.3 ms script + 0.5 ms compositor commit).
- The one idle drop: thought-sphere fitted to the screen at DPR 2, 31 charts, 56.6 fps at 38 %
  busy: canvas commit 11.5 %, script 10.2 %, task/IPC 14.6 %, layout 1.1 %, style 0.
- The one measured interaction drop: inspector open at 30 nodes, 60 → 50–52 fps (31–45 under
  load). Two causes, isolated by removing each: `backdrop-filter: blur(8px)` on the side pane
  (removing it alone restores 56–59 fps) and the metadata panel rewriting DOM text on every data
  frame and forcing a layout per flush (removing it alone leaves 31–40 fps).
- Every document patch costs the frontend ~0.2–0.25 ms PER NODE: 1.7 ms at 5 nodes, 7.3 ms at 30,
  11 ms at 60. A slider or knob emits one patch per pointer event (60–125/s). At 30 nodes that is
  ~45 % of the main thread for the length of the drag; at 100 nodes it saturates.
- The backend's write path holds the graph lock for 9 ms at 30 nodes, 30 ms at 100, 65 ms on
  music.gfi (4764 params) in the debug build. A 2 s knob drag at 60 Hz on music.gfi backlogged
  4.5 s, overflowed the 256-slot control ring 29 times and re-seeded the page with 24 MB of
  `hello` — ~29 long tasks of 100–150 ms. Release estimate: 7–13 ms per op, at the edge.
- `/data` is the dominant traffic whenever anything is visible (71 KB/s for 31 scalar viewers,
  1.55 MB/s on the audio patch); `/control` is 2 messages per node per 500 ms regardless of
  visibility (125 msgs/s for 35 nodes); `doc_patch` was 0/s idle on every patch. A hidden tab
  drops `/data` to 0; off-screen viewers do not subscribe; layout and style are not a cost.

## Why it slows down as patches grow

Three mechanisms with three triggers, which is why it does not feel like one thing:

1. **Continuously, in proportion to visible charts × DPR².** Each is a canvas with its own
   compositor commit and a full uPlot redraw per delivered frame, even for a 99-byte scalar. On
   a HiDPI display ~20–25 visible charts saturate. Owned by `viewer-render-surface.md`; the cuts
   below buy time.
2. **While editing, in proportion to ALL nodes.** Every `doc_patch` re-assembles every node into
   deep `$state`, and a gesture emits a patch per pointer event; the backend runs five whole-graph
   passes per op under the graph lock and, when it falls behind, re-seeds every tab. This is the
   "it gets worse with complexity" the owner feels while turning a knob on a big patch.
3. **With the inspector open, a constant tax** of 3–15 ms per composited frame, which stacks on
   1 and 2 and is present in the normal editing posture.

## Tier 1 — the causes, in order

Each item names its anchor, the fix the verifiers converged on, and the tests to extend. None
adds a path; several delete one.

1. **Patch-scoped document sync** — `frontend/src/lib/crdt/syncClient.ts:28,60`,
   `stores/graph.svelte.ts:191–201, 773, 784–792, 859–876`. `onDocChange` passes the applied
   patch's top-level keys (and the uids under `nodes`); `_syncFromDoc` re-derives only the roots
   named, so a `variables`-only or `arrangement`-only patch touches no node. `nodeById` becomes an
   O(1) read of a `Map` written by the one writer of `nodes` (`_reconcileNodes`), replacing the
   linear find that made the reconcile, every `SlotViewer` and every control event O(N).
   `_reconcileNodes` writes per field only when the value differs: `pos` by value, `params` leaves
   in place (as `applyLiveParams` already does), `viewers`/`baseline` only when the raw string
   changed, `this.nodes` identity kept unless membership changed. `nodeTypes` becomes
   `$state.raw`. Tests: `graphUpdateParam` gains "a variables-only patch leaves node, params, pos
   and viewers identities untouched" and "a member scope patch refreshes its facade";
   `syncClient.test.ts`; e2e `variables-layout`, `integrity`, `touch`, `midi-learn`.
2. **One owner of a gesture's send rate** — `frontend/src/lib/ui/liveValue.svelte.ts:38–41`,
   `Knob.svelte:47`, `Slider.svelte:62`, `NumberInput.svelte:91–103`. `commit(v)` keeps the newest
   value, sends latest-wins at most every ~33–50 ms with one op in flight, skips a value equal to
   the last sent, and `end()` always flushes. Knob goes through `useLiveValue` like the other two.
   Backend follow-up in the same change: `arms.rs:729` stops echoing the node's full descriptors
   (3.6 KB) on a plain value edit — the patch carries the value and `/params` carries the error.
   Tests: `liveValue.test.ts` with fake timers (N commits in 50 ms → one send; `end()` flushes);
   e2e `touch` (knob drag), `harmonic-geometry` and `midi-learn` (sliders).
3. **The write path off the lock** — `backend/goofi-bridge/src/lib.rs:1492–1501, 1431–1437`,
   `doc.rs:97–102`. Delete the per-op YAML manifest: autosave serializes when its own tick finds
   the graph dirty (it already wakes on `changed`), and save/patchfile read the graph the same
   way. `reconcile_root` takes the projection by value and `mem::replace`s it instead of cloning.
   Build the projection under the graph lock, take the doc lock while still holding it (the order
   that keeps apply→re-project atomic), then release the graph lock before the diff, the version
   bump and the encode. Run `dispatch` off the socket task (one ordered worker per control socket,
   the shape `phrase::exec_lines` already uses) so a slow op no longer parks that socket's event
   drain — this is what turns a backlog into a re-seed storm. Tests: `browser` situations
   (tab mirrors the graph; three devices converge; a tab that fell behind is re-seeded),
   `editing` stale-toggle convergence, `session` crash-leaves-autosave; e2e `recover`,
   `save-options`, `socket`.
4. **The follower paced and made cheap** — `lib.rs:1448–1478`, `goofi-graph/src/lib.rs:614–620,
   737–800, 2537–2665`. `recv_timeout(BROADCAST_PERIOD)` returns on the FIRST value, so a followed
   30 Hz slot re-projects and rebroadcasts the whole document 30 times a second (60 is the cap,
   per slot). After the first value, drain for one `VIEWER_INTERVAL` into a per-variable
   latest-wins map, then apply once. Split `rebind` so a moving VALUE refreshes `resolve_vars` on
   the existing record and keeps the compiled expression: today every follower write releases and
   recompiles the Python expression of every reader, under the GIL, under the graph lock. With
   item 1 the frontend then ignores the node tree for a variables-only patch. Tests: `editing`
   (source taken, edit refused, clear sticks), `session` (a source rides the archive); e2e
   `midi-learn`.
5. **The inspector's two taxes** — `frontend/src/lib/panels/SidePane.svelte:121–122`: replace the
   translucent `color-mix` + `backdrop-filter: blur(8px)` with `background: var(--surface-1)`;
   drop the blur from `AddNodeMenu.svelte:218` and `ViewerSettingsMenu.svelte:146` too unless the
   frosted look is wanted on those two transient menus. `editor/MetadataPanel.svelte:17,27–34,
   47–53`: the bound callback stays (the stream must be served) but writes nothing; the existing
   250 ms interval publishes `lastFrame = latestFrame(node, slot)` as `$state.raw` beside
   `drops`. `MetadataField.svelte:17–20` loses the `firstLine; measure()` effect that forced a
   layout inside the flush. Tests: e2e `metadata` (auto-retrying, passes at 250 ms), `integrity`.
6. **The viewer chrome** — `viewers/ViewerFeed.svelte:17`: `frame` becomes `$state.raw` (a frame
   is an immutable payload; the deep proxy is allocated per frame per viewer today).
   `api/frames.ts:77–91` + `viewers/ArrayViewer.svelte:223,250`: wrap the per-slot callbacks in
   `flushSync` and uPlot's `setData` in `plot.batch`, so the 8 ms budget and the most-starved
   order measure real draw work — today they measure microseconds and never engage, because
   every draw runs in a microtask after the rAF returns. `ArrayViewer.svelte:142–152, 76–116`:
   hide uPlot's axes (they draw no labels here, only grid work per redraw), memoize the corner
   tick strings on the range, and skip the redraw when the payload is byte-identical to the last
   drawn. `viewers/capacity.ts:94`: ask for at most 32 rows for a line viewer instead of
   `min(h, 512)` — 32 overlapping traces saturate a 133 px plot, and a 64-channel viewer at DPR 2
   is a 20 ms long task on its own today; `envelope.ts:32` passes `Float32Array` rows through
   instead of copying to number arrays. Tests: `frames.test.ts` gains a slow-consumer case
   asserting the second slot defers; `capacity.test.ts` row expectation; e2e `socket` viewer
   cases; a manual look at thought-sphere fitted at DPR 2.
7. **Zoom-aware viewer demand** — `viewers/ViewerFeed.svelte:35–47`, `editor/GoofiNode.svelte`.
   The box a viewer declares is `contentRect × DPR`, blind to the flow zoom, and the visibility
   gate is an `IntersectionObserver` that sees every card at low zoom: zooming out turns hidden
   viewers into full-size streams. Read the zoom (`useViewport()` inside the flow; 1 in a docked
   panel), derive the box from `w × zoom`, and unsubscribe below a readability threshold
   (~0.3, a 66×40 px card) while keeping the last frame as a frozen thumbnail. Hysteresis across
   the 32 px steps so a pinch does not renegotiate every slot per step. Tests: e2e `socket`
   (frames arrive; a second viewer on one slot) at zoom 0.85–1 must still pass; `touch` gains a
   pinch that crosses the threshold and asserts `/data` goes quiet and resumes.
8. **The dev profile** — `Cargo.toml:50–54`: `opt-level = 1` for the workspace and `opt-level =
   2` for dependencies, keeping line tables and debug assertions. Measured 5.8× on the scalar
   serve path, 7.5× on audio frames, 8.8× on a 512² image; the status drain drops from 15–20 % of
   a core to ~2 %. The `pulseaudio`/`goofi-audio` overrides stay. This does not change the
   frontend's own numbers; it changes whether the backend keeps up with item 2's 20 Hz.

## Tier 2 — real, smaller, do when touching the file

- **Control plane: one message per period, not two per node** — `lib.rs:650–660`: one
  `node_stats` carrying `{uid: hz}` and one `param_values` carrying `{uid: pair}`, still restated
  whole; `graph.svelte.ts:326–333` iterates the maps through the uid index and writes
  `stats.updates_per_second` in place. Removes the O(N) burst that overflows the ring, the
  per-message parse/dispatch, and 89–177 messages/s of idle traffic.
- **Data worker: decode and post on arrival** — `api/dataWorker.ts:21,43,100–118`: delete the
  16 ms tick, `latestRaw` and `DemandTicker`; the WS handler decodes and posts at once. The
  backend already caps a slot at 30/s and `frames.ts` already coalesces under the paint cap, so
  the worker's own latest-wins stage was a second owner that silently dropped 10–19 % of frames
  at 30–60 viewers and added up to 16 ms of latency. (Supersedes "one post per tick" in
  `data-plane-bandwidth.md` and `viewer-render-surface.md`.)
- **Node drag** — `NodeEditorPanel.svelte:515–558, 673–676`: cache drop-zone and panel rects at
  drag start instead of three document-wide `querySelectorAll` + `getBoundingClientRect` per
  pointer move; pass the flow node to `boundsOf` instead of a `find` per target (O(N²) → O(N));
  send a multi-node move as ONE `compound` op (the backend already resyncs once per batch).
- **Flow rebuild on selection** — `NodeEditorPanel.svelte:321–369`: reuse the previous flow
  node/edge object when nothing it carries changed, so xyflow's identity checks short-circuit;
  a click in one panel currently rebuilds every flow object in every editor.
- **TopBar overflow replan** — `editor/TopBar.svelte:239–250`: key the effect on a string of tab
  names + active id, not on `ws.state.workspaces` identity (two forced layouts per doc patch on a
  narrow layout today).
- **Viewpoint push** — `arms.rs:825–836`: `layout viewpoint edit` runs the whole write path for a
  value that is not in the document; return after `set_viewpoint`.
- **`/params` restates unchanged pairs** — `lib.rs:1721–1731`: keep the last sent text and skip
  an identical send (20 lock takes and sends per second per open inspector).
- **Reducer threads sleep on a channel** — `reducer.rs:302`: one OS thread per ever-watched slot
  wakes 60×/s for the node's lifetime; `recv_timeout` on a per-reducer poke, `REDUCER_TICK`
  while wanted and `REHOME_INTERVAL` when idle. A joiner's first serve also stops being a tick
  late.
- **Status drain pacing** — `lib.rs:585–596`: the drain wakes per report notify (470/s with a few
  time-varying expressions); take the lock at most once per `LIVE_PERIOD`.
- **Topomap on a fixed field grid** — `viewers/TopomapViewer.svelte:115–184`: evaluate the field
  into a 128² offscreen canvas per frame and `drawImage` it scaled; today it is O(pixels ×
  channels) at canvas size, 21–41 ms per frame in a docked panel.
- **Image viewers and the context cap** — `viewers/imageGL.ts:174–179`, `ImageViewer.svelte`:
  `loseContext()` on dispose, `webglcontextlost`/`restored` handlers; the 17th live image viewer
  blanks the oldest permanently today. Goes away with the render surface.
- **TABLE frames are never reduced** and the undeclared plan caps only dims 0 and −1 —
  `goofi-core/src/reduce.rs:23`, `reducer.rs:36`: reduce nested arrays with the same plan; bound
  the undeclared preview by a total element budget in `goofi_view::plan`.
- **Kernel allocations** — `reduce.rs:255` (two `Vec`s per bin per row in `envelope_axis`),
  `codec/decode.ts:131` (the `|u1` body needs no copy), `viewers/envelope.ts:31`, the console
  store's eviction scan (`console.svelte.ts:51`), `flash`/`variableDrag` as `$state.raw`,
  `SubpatchZoomExit` bounds as a derived, the virtual-cables panel skipping an unchanged reply.

## Refuted — do not re-investigate

- "The reducer decodes every producer sample it drains": the subscriber is one-deep with safe
  overflow, so it drains at most one sample per tick.
- "Per-frame `clientWidth` reads force layout after the same microtask's writes": Svelte's batch
  runs render effects before user effects, and the read sites are not reached by envelope or
  scalar frames.
- "uPlot's cursor-point divs are compositor layers per series": they carry `display: none` until
  hovered.

## Not reproduced, and what would settle it

The idle drop below 30 fps at ~50 nodes did not appear headless. The measured per-redraw cost
predicts it on a HiDPI display with 20–25 visible charts, and the editing and inspector costs
above stack on it. Before the render surface lands, one measurement on the owner's own display
settles the split: Chrome's Performance panel over 5 s on thought-sphere fitted to the screen,
once idle, once with the inspector open, once during a knob drag; the shares to read are
Commit/ProduceCanvasResource, FunctionCall, Layout, and long tasks. Note the DPR and whether a
native graphics window was open — a GPU shared with the engine is the one cost this audit could
not see.

## Found beside the audit

- **A relative `GOOFI_BUILD_DIR` breaks every native node build**: the generated crate's path
  dependencies are written relative and cargo resolves them against the crate directory
  (`crates/<hash>/target/goofi-build/sdk/...`). `goofi-build` must canonicalize the directory
  once at the boundary. The e2e fleet passes an absolute path and never met it.
- `FitToGraph.svelte` fits the camera to whatever graph the page loads with; a wholesale reset
  can arm a fit against nodes about to be replaced. Minor, but it made two measurement runs
  incomparable.
