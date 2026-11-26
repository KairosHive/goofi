# plotluck

Many plots, one WebGL2 canvas.

plotluck maps points and image data onto a canvas as fast as the GPU takes an upload. A page with
thirty small charts usually pays thirty times for a canvas commit, a path build and a main-thread
task. plotluck pays once: every plot is a rectangle and a GPU buffer, a new frame is a buffer upload,
and one animation frame draws every visible plot.

```ts
import { createRenderer } from '$lib/plotluck';

const renderer = createRenderer(); // throws without WebGL2; a worker can own it
const surface = renderer.surface(canvas); // any canvas, or an OffscreenCanvas handed to the worker
surface.setView({ x: 0, y: 0, zoom: 1, width, height, dpr: devicePixelRatio });

const line = surface.addLine();
line.setRect(20, 20, 320, 160);
line.push({ rows: [samples] });
```

It owns pixels and nothing else: no data sources, no sockets, no DOM, no text, no dependencies.
What a plot shows, when it updates and how it is labelled are yours.

plotluck is a part of goofi's frontend library. It imports nothing from the rest of goofi.

## The renderer and its surfaces

A renderer is one WebGL2 context, and every surface it makes is drawn by it: a browser allows a
thread sixteen contexts and a worker four, so a page of panels cannot pay one per canvas. A frame
draws the surfaces that changed side by side on one atlas canvas, and one bitmap handover brings
each surface its picture on its own canvas, which holds a 2D context for that. A frame draws only
the rects that changed: a plot that took a push is drawn again with the plots that overlap it, a
plot that moved also clears where it was, and a new view draws the whole pane. A surface that did
not change costs nothing.

`setView` takes the pane in CSS pixels, a camera in world units and the device pixel ratio. Every
plot's rect is in world units, so a host with a pan-and-zoom canvas moves the camera and the plots
follow; a host without one passes `zoom: 1` and `x = y = 0`. Plots outside the pane are culled, a
lost context is restored, and overlapping plots draw in `setOrder(z)` order. A plot clears its rect
to its background first; one whose background is transparent draws over what is already there.

## Line plots

```ts
line.setBackground('#111111');
line.setSettings({ logX, logY, yAuto, yMin, yMax, points });
line.push({ rows: [Float32Array, ...], xs?, base? });
line.range();  // { xMin, xMax, yMin, yMax, scalar }: the numbers for your own labels
line.clear();  // background alone until the next push
line.remove();
```

Every row is one series, and every series is one instanced draw of line segments. A single row of
length 1 is a scalar: a bar at `x = value` over a running range. Rows of interleaved min/max pairs
with two `xs` per pair draw as the band they describe, so a decimated signal keeps its envelope.
`xs` may instead hold one row per series, and with `square` set both axes share one window: a
trajectory or a phase portrait, drawn without distortion.

The grid follows the window: 1-2-5 steps on a linear axis, decades with fainter mantissa lines on
a log axis, each axis on its own.

Series colours are procedural. `seriesColor(i)` takes the hue that bisects the widest arc left by
the series before it, at one OKLCH lightness and chroma per ring of eight, so any count stays
distinct and a series keeps its colour when more are added. Use it for your legends.

## Image plots

```ts
const image = surface.addImage();
image.setSettings({ lut: Uint8Array /* 256×3 */, stretch });
image.push({ values, width, height, channels, lo, hi });
```

One channel goes through the LUT, two draw as red and green, three as RGB, four as RGBA. An image
keeps its aspect ratio inside its rect unless `stretch` is set.

## Path plots

```ts
const path = surface.addPath();
path.push(paths().stroke([x0, y0, x1, y1, ...], width, '#c5c8d6').dot(x, y, diameter, rgba).build());
```

Strokes and round dots the host lays out itself, in fractions of the rect (x right, y down), each
with a width and colour of its own: a diagram, a projection, a scaffold around another plot.

## Field plots

```ts
const field = surface.addField();
field.setSettings({ lut, bands: 0 });
field.push({ points, affine, frame, disc, lo, hi });
```

A thin-plate spline over up to 128 weighted centres, read at every fragment of a disc and mapped
through the LUT: a scalp map, a heat map over sparse sensors. The host solves the weights; the
surface evaluates the field at the pixel, so the picture is as sharp as the rect is large.
`bands` posterizes the window so the boundaries read as contour lines.

## Tests

`npm --prefix frontend run test` runs the pure parts: rects, windows, grids, layout, palette.
