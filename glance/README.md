# glance

One WebGL2 canvas that draws many plots. A host gives it rectangles and typed arrays; it owns
nothing else: no frames, no sockets, no CSS variables, no dependencies.

## Why

A page with thirty small charts pays once per chart for a canvas commit, a path build and a
main-thread task. glance draws every plot into one surface in one animation frame: a plot is a
rectangle plus a GPU buffer, and a new frame is a buffer upload, not a state write.

## API

```ts
import { createSurface } from 'glance';

const surface = createSurface(canvas);          // throws without WebGL2
surface.setView({ x, y, zoom, width, height, dpr });

const line = surface.addLine();
line.setRect(x, y, w, h);                        // flow units; drawn at rect × zoom × dpr
line.setBackground('#111111');
line.setSettings({ logX, logY, yAuto, yMin, yMax, points });
line.push({ rows: [Float32Array, ...], xs?, base? });
line.range();                                    // { xMin, xMax, yMin, yMax, scalar } for labels
line.setOrder(z);                                // overlapping plots draw in z order
line.clear();                                    // background alone until the next push
line.remove();

const image = surface.addImage();
image.setSettings({ lut: Uint8Array /* 256×3 */, stretch });
image.push({ values, width, height, channels, lo, hi }); // 1: LUT, 2: red+green, 3: rgb, 4: rgba

surface.dispose();
```

`setView` takes the pane in CSS pixels, the camera in flow units and the device pixel ratio; a
host with no camera passes `zoom: 1` and `x = y = 0`. Every plot's rect is in flow units, so a
plot follows a pan or zoom when the view changes and the rects do not.

Series colours are procedural: `seriesColor(i)` takes the hue that bisects the widest arc left by
the series before it, at one OKLCH lightness and chroma per ring of eight, so any count stays
distinct and earlier series keep their colour; a host uses it for its own legends.

A single row of length 1 is a scalar: it draws as a bar at `x = value` over a running range.
Rows of interleaved min/max pairs with two `xs` per pair draw as the band they describe.

The grid follows the window: 1-2-5 steps on a linear axis, decades with fainter mantissa lines on
a log axis, each axis on its own. Text is not drawn. `range()` gives the numbers the host puts in its own labels.

## How goofi uses it

The node editor keeps one surface as the first child of the SvelteFlow node layer, repositioned in
flow units on every camera change, and each viewer body is a transparent rect the surface paints
into. A docked viewer panel keeps a surface of its own at zoom 1.

## Develop

```sh
npm install
npm test      # vitest over the pure parts: rects, windows, layout, palette
npm run check
```
