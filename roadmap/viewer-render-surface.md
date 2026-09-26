# Viewer rendering: one GPU surface per panel

Viewers draw on glance (`github.com/dav0dea/glance`), hand-written WebGL2 with no canvas-2D
path and no WebGPU backend (uneven coverage on Firefox, Linux Chrome and older iPads; headless
Chromium needs SwiftShader flags). The surface owns pixels only; text stays in the DOM.

## Remaining

1. Topomap and trajectory onto the surface: the interpolation (`topomapInterp.ts`, CPU today)
   becomes a fragment shader over the electrode positions; the trajectory a line plot with two
   series per pair. Then `ViewerSurface` keeps only the string and table kinds.
2. `glance` from npm: `frontend/package.json` takes it from its GitHub repo today; when it is
   published, the dependency takes a version.
3. Worker-side rendering: `dataWorker.ts` owns the sockets and the decode; with
   `transferControlToOffscreen` it draws straight from the decoded buffer and the main thread
   sends rects and the camera on change. The renderer runs on either thread already.
