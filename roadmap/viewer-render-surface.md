# Viewer rendering: one GPU surface per panel

Viewers draw on plotluck (`github.com/dav0dea/plotluck`), hand-written WebGL2 with no canvas-2D
path and no WebGPU backend (uneven coverage on Firefox, Linux Chrome and older iPads; headless
Chromium needs SwiftShader flags). The surface owns pixels only; text stays in the DOM.

## Remaining

1. Worker-side rendering: `dataWorker.ts` owns the sockets and the decode; with
   `transferControlToOffscreen` it draws straight from the decoded buffer and the main thread
   sends rects and the camera on change. The renderer runs on either thread already.
