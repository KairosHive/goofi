# Harmonic Geometry Patchwork Cookbook

Open **[Cookbook.html](Cookbook.html)** for the illustrated guide, nine recipes,
control notes, a 46-form atlas, and the node reference.

Start goofi from this checkout, then load a `.gfi` file with the app's file browser.
Each patch has a **Play** tab with controls and a **Patch** tab with its cables.
Extra tabs show the shader or a second analysis view. Turn **auto** off to use **mix**.

```sh
cargo run -p goofi-init
cargo run -- --load examples/harmonic-geometry/01-breathing-lines.gfi
```

| Patch | Explore |
| --- | --- |
| [01 · Breathing lines](01-breathing-lines.gfi) | Continuous tuning, phase, stretch, damping, harmonographs, GPU Lissajous |
| [02 · Plate mode walk](02-plate-mode-walk.gfi) | Integer endpoint modes, fractional mode interpolation, plate-to-wave shader |
| [03 · Between media](03-between-media.gfi) | Circular plate to open quasicrystal; separate tuning and field blend |
| [04 · Sand and memory](04-sand-and-memory.gfi) | Nodal sand, antinodal powder, tracer flow, persistent GPU ink |
| [05 · Knot to knot](05-knot-to-knot.gfi) | Fixed mesh correspondence, camera motion, tube radius |
| [06 · Interval garden](06-interval-garden.gfi) | Graphs, fractals, BioColors palettes, geometry metrics, EuclidRhythm |
| [07 · Peaks to worlds](07-peaks-to-worlds.gfi) | Synthetic signal → HarmonicSpectrum → geometry; Tuning and TimbreControls |
| [08 · A common chord](08-common-chord.gfi) | Shared component fades across geometry and continuous native synth controls |
| [09 · Jade resonance](09-jade-resonance.gfi) | Full-window Chladni relief; morph jade, brushed metal, woven silk, and porous stone |

Recipes 01–08 use 256-pixel GPU previews and 960 × 660 CPU dashboards.
Recipe 09 uses a 512-pixel field and material. They open
no audio device or native window. Recipe 08 includes the complete synth chain
through `mixdown.out`; add and connect `audio:AudioOut` when you want to listen.

The Python geometry nodes require the same pinned Biotuner revision as the
existing bundle. The app's default build includes the Python expression evaluator
used by these controls. A Rust test build needs `--features embed` for recipe tests.

Read the [source survey and design plan](../../node-bundles/harmonic-geometry/SURVEY.md)
and the [bundle interface](../../node-bundles/harmonic-geometry/README.md).
`build_patches.py` rebuilds the archives. `build_images.py` converts public-test
frames to the illustrations. `build_cookbook.py` rebuilds the HTML guide.
The [verification record](VERIFICATION.md) gives checks and rebuild commands.
Only recipe 07 contains a custom node: the supplied synthetic signal source.

For the Chladni material study, load **09 · Jade resonance**. **Canvas** fills
the viewing area; **Play** shows the controls. Canvas stretches the image to
the panel, including in portrait. The shader extends the field with mirrored
tiles, so camera rotation and tilt leave no plate outline or empty border.

Select **textureA** and **textureB** from jade, brushed metal, woven silk, and
porous stone. **textureAuto** moves between them at a separate rate from the
mode walk. Turn it off to set **textureMix** by hand. The blend changes small
surface heights, color, and reflection together. **textureScale** sets detail
size; **textureDepth** sets the strength of grooves, threads, and pores.

Turn **auto** off to hold the harmonic structure with **mix**. Increase relief
to see the raised seams; move light and tilt to inspect their shape. Roughness
controls the highlights. Patina changes the jade finish from bronze to green.
The node also has contour count, turn, zoom, and exposure controls. Material
and camera controls leave the upstream harmonic field unchanged.
