# Viewer rendering: one GPU surface per panel

Decided 2026-09-16 from the performance audit. The owner reports the browser's frame rate falling
as a patch grows. The audit measured the shipped viewers on the owner's own patches and found the
cost in HOW the viewers paint, not in what they are sent. This file is the render architecture
that replaced uPlot and the per-viewer canvases (`glance/`), and what remains of it.

## What was measured

Headless Chromium, 1600×1000, debug backend, `test-patches/*.gfi` (2026-09-16):

- `thought-sphere` — 35 nodes, 31 inline viewers on screen at DPR 2: 56.6 fps. The main thread
  was 38 % busy: **canvas commit 11.5 %** (`ProduceCanvasResource` + compositor commit, one per
  `<canvas>` per redraw), **script 10.2 %** (uPlot `setData` → path building → stroke,
  `drawCornerTicks` `fillText` ×4, the Svelte flush behind `ViewerFeed`'s `frame = f`), **task and
  IPC overhead 14.6 %** (one `postMessage` + structured clone per frame per viewer, ~0.13 ms
  each), layout 1.1 %, style ~0. At DPR 1 with 24 charts on screen: 22 % busy.
- One redraw costs ~0.8 ms at DPR 2, commit included. The cost is `visible charts × redraw rate
  × DPR²`, which puts saturation near 20–25 visible charts at 30 Hz on a HiDPI laptop — the
  machine the owner sees it on, and one headless at DPR 1 does not reach.
- A 99-byte scalar frame costs the same full uPlot redraw as a 4000-point one.
- Every image viewer holds a WebGL2 context of its own (`viewers/imageGL.ts`); browsers evict the
  oldest past ~16 live contexts.
- Layout and style are NOT where the time goes. The doc-sync path (`_syncFromDoc`) is heavy per
  patch but fires only on edits; `doc_patch` was 0/s idle in every scenario.

uPlot is close to the floor for canvas-2D. What remains is the canvas-2D model itself: N canvases,
each a compositor layer with its own commit, JS building every path, DPR² pixels per redraw, and a
main-thread task per frame per viewer. The lever is **N canvases → one surface** and **N
main-thread deliveries per tick → one, later zero**.

## Decisions

**A Svelte package beside `panelty`, owning nothing but pixels.** It takes typed arrays and
rectangles and draws; it knows no frames, no sockets, no stores. goofi binds it in `ViewerFeed`
behind the same `ViewBinding` the viewers use today, so kind and settings keep their owner.
Working name `glance`; the name is open.

**WebGL2, hand-written, not three.js.** Three's scene graph is fine for a hundred draw calls but
wrong for this: its `Line` is 1-px `GL_LINES`, `Line2` rebuilds instanced geometry on the CPU on
every `setPositions`, and it is ~600 KB for what is 1.5–2 k lines of purpose-built code. A tiny
helper such as `twgl` for boilerplate is acceptable. WebGL2 is required; there is no canvas-2D
fallback, because two render paths would be two owners of one picture.

**Not WebGPU, for now.** Nothing here needs what it adds: the reductions happen in the bridge, a
few hundred instanced draws are nothing for WebGL2, `f16` attributes and textures are core in
both, and the per-canvas commit cost is removed by the single surface under either API. What it
would cost is real: Firefox and Linux Chrome coverage still depends on version and drivers, iPads
below iPadOS 26 have none, and headless Chromium needs SwiftShader flags for it, which makes the
Playwright pixel checks flakier. The one argument for it is WGSL, which the graphics engine
already speaks. So the renderer is written against a small backend seam — buffers, regions,
pipelines, instanced draws, resize, lose/restore — implemented on WebGL2; a WebGPU backend is a
later addition behind the same seam. The feature that asks for it is already named: a graphics
engine running in the browser draws its output into a viewer rect zero-copy only on a shared
WebGPU device (`engines-in-the-browser.md`).

**One canvas per editor panel, sized in device pixels, positioned in flow units.** The surface is a
single `<canvas>` inserted as the first child of SvelteFlow's node layer, so DOM order gives the
stacking for free: edges stay behind it, cards paint over it, a raised card occludes the plot of
the card beneath it. The card leaves its viewer body TRANSPARENT and the surface paints the body's
background and the plot inside that rect. Because the node layer carries the camera transform,
the canvas is repositioned on every camera change — `left/top/width/height` in flow units, one
style write, the same thing SvelteFlow does to its own viewport — while its backing store stays
`pane × DPR`, so a plot is always drawn 1:1 in device pixels at any zoom. Rejected: a canvas in
`ViewportPortal target="back"` (edges would draw over the plots), one in `target="front"` (it
would cover a raised card's header and needs stencil occlusion from node rects), one outside the
transformed viewport (it cannot interleave with the edge and node layers).

**A plot is a rect plus a series buffer; a frame is a buffer upload, never a state write.** The
surface exposes `addPlot(rect, kind, settings) → handle`, `handle.push(values, shape)`,
`handle.setRect`, `handle.setSettings`, `remove()`. `push` writes into a
per-plot region of one shared VBO (or a float texture) and marks the surface dirty. One `rAF` per
surface draws every visible plot when anything is dirty — a draw call each, so redrawing all is
cheaper than tracking damage. `frames.ts` stays the registry and the paint cap; the consumer of
`bindViewer` becomes `handle.push`, and `ViewerFeed` stops writing `frame` into `$state`.

**Lines are instanced quads, decimated on the GPU.** Segment-per-instance with width ≥ 1 px and
proper joins for the few thousand points a viewer draws; the min/max envelope the bridge already
serves at viewer width (`capacity.ts`) arrives as pairs and is drawn as a band directly. Y
autoscale is a per-plot reduction of the uploaded region (a small pass or a CPU min/max over the
typed array — the cost is the same, the CPU form is simpler). Log scales are a uniform.

**Text stays in the DOM.** The two to four range labels a plot carries are `<span>`s in the slot
viewer, updated only when the range changes. A glyph atlas is not worth its code here.

**Culling has one owner.** The surface knows the camera and every rect, so it skips plots outside
the pane. `ViewerFeed`'s `IntersectionObserver` currently decides both painting and subscription;
once the surface culls, visibility comes from it and the observer goes.

**Images move onto the same surface.** An image is a `u8` texture in a rect; `imageGL.ts`'s
per-viewer context and its 2-D fallback canvas are removed, which also ends the 16-context limit.
The topomap's interpolation (`topomapInterp.ts`, CPU today) becomes a fragment shader over the
electrode positions; the trajectory is a line plot with two series per pair.

**Render in the data worker once the renderer is stable.** `dataWorker.ts` already owns the
sockets and the decode. With `transferControlToOffscreen` it can draw straight from the decoded
buffer: no `postMessage` per frame, no structured clone, no Svelte flush, no main-thread task at
all. The main thread then sends rects and the camera on change and asks for readouts on hover.
The renderer is written to run on either thread; it ships on the main thread first.

**Context loss is handled, not avoided.** The series data lives in JS, so `webglcontextlost` →
`webglcontextrestored` re-creates buffers from it and redraws.

## Before the package: the cheap cuts

These fall out of the audit (`frontend-performance.md` has the full list with anchors) and are
worth doing in the current viewers, since the package will take weeks and each is under a day:

- Skip the redraw when a frame is byte-identical to the last one drawn — a held scalar at 30 Hz
  redraws nothing new.
- Draw the corner tick labels only when the scale changed, not in every draw hook; hide uPlot's
  axes, which draw no labels here but do grid work per redraw.
- Make `flush()` deliver synchronously (`flushSync` + `plot.batch`) so the 8 ms budget measures
  real draw work; today every draw runs in a microtask after the rAF returns and the budget
  never engages.
- Store the delivered frame as `$state.raw`, and size the declared box by the flow zoom, so a
  zoomed-out canvas does not stream and redraw full-size viewers nobody can read.
- Let the worker decode and post on arrival and delete its 16 ms tick (`dataWorker.ts` `drain`,
  `DemandTicker`): `frames.ts` is the one latest-wins owner (`data-plane-bandwidth.md`).

## Done

The cheap cuts; `glance/` (surface, line and image plots, hit test, vitest over the pure parts,
`tests/e2e/tests/viewer.spec.ts` for the pixels); `ArrayViewer` and `ImageViewer` migrated behind
`ViewBinding`, uPlot and `imageGL.ts` removed. The docked `viewer` panel keeps a surface of its own
at zoom 1.

## Remaining

1. Topomap and trajectory onto the surface: the interpolation (`topomapInterp.ts`, CPU today) as a
   fragment shader over the electrode positions; the trajectory as a line plot with two series per
   pair. Then `ViewerSurface` keeps only the string and table kinds.
2. Worker-side rendering; hover readouts become a round trip.
3. A WebGPU backend behind the same seam, when the browser graphics engine wants the shared device.

## Accepted

- Occlusion is by DOM order and plot z order: a raised card covers the plot beneath it because its
  plot draws later with an opaque background. Two unraised cards with equal z stack by DOM order
  while their plots draw in insertion order, so a body dragged over a neighbour's body can show the
  neighbour's plot until one card is raised.
- The body's padding is an opaque frame in the card colour, so the plot stays inside the card's
  rounded corners and the pane never shows through it.
- `.goofi-node.booting .surface { opacity }` no longer dims a line or image plot: its pixels are on
  the panel surface under the card, not in the card's DOM.
