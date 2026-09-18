# goofi-graphics-web

The graphics engine compiled to WebAssembly, drawing on WebGPU. This crate is glue: it hands the
page's canvas and clock to `goofi_graphics::web` and exports `start` and `Stage.tick`.

Build once per engine change:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.127   # the version in Cargo.lock
backend/graphics/goofi-graphics-web/build.sh
```

The output lands in `frontend/static/dev/graphics/` (gitignored). The dev page `/dev/graphics`
loads it and draws one shipped generator; it says so in one line when the output is missing or
the browser has no `navigator.gpu`.
