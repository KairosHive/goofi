# Viewer rendering: one GPU surface per panel

Decided 2026-09-16 from the performance audit. Viewers draw on `glance/` (one WebGL2 canvas per
editor panel and per docked viewer panel). This file holds what remains and the decisions that
bind it.

## Decisions that hold

- **Every viewer draws through the surface.** No canvas-2D path beyond the two remaining kinds,
  and none comes back: two render paths would be two owners of one picture. A browser without
  WebGL2 sees the text fallback.
- **WebGL2, hand-written.** No WebGPU backend: nothing here needs it, its coverage on Firefox, Linux
  Chrome and older iPads is uneven, and headless Chromium needs SwiftShader flags for it. Engines
  do not run in the browser, so the one feature that asked for a shared WebGPU device is gone.
- **The surface owns pixels only.** It takes rects in flow units and typed arrays; kinds, settings,
  frames and sockets stay in goofi. Text stays in the DOM: the range labels are spans, shown on
  hover.
- **A frame is a buffer upload.** `push` writes the plot's GPU region and marks the surface dirty;
  one animation frame draws every visible plot. Culling and context loss are the surface's.
- **Series colours are procedural**: the hue that bisects the widest arc left by the earlier series,
  at one OKLCH lightness and chroma per ring of eight, so any channel count stays distinct and a
  series keeps its colour when channels are added.
- **Occlusion is by DOM order and plot z order.** A raised card covers the plot beneath it because
  its plot draws later with an opaque background. Two unraised cards of equal z stack by DOM order
  while their plots draw in insertion order; raising a card fixes the picture. Accepted.
- **The body's padding is an opaque frame in the card colour**, so the plot stays inside the card's
  rounded corners. `.goofi-node.booting .surface { opacity }` does not dim a plot: its pixels are
  on the panel surface under the card. Accepted.

## Remaining

1. **Topomap and trajectory onto the surface.** The interpolation (`topomapInterp.ts`, CPU today)
   becomes a fragment shader over the electrode positions; the trajectory a line plot with two
   series per pair. Then `ViewerSurface` keeps only the string and table kinds.
2. **`glance` as an npm package.** It is linked from the repository root today, which needs
   `server.fs.allow` in `frontend/vite.config.ts` and `../glance/src` among the bridge's SPA
   inputs. When it is published, both go and `frontend/package.json` takes a version.
3. **Worker-side rendering.** `dataWorker.ts` owns the sockets and the decode; with
   `transferControlToOffscreen` it draws straight from the decoded buffer and the main thread sends
   rects and the camera on change. The renderer runs on either thread already.
