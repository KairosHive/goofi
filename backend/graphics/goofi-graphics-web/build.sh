#!/bin/sh
# The browser graphics engine, built and bound for the page. Output is not committed.
set -eu
root=$(cd "$(dirname "$0")/../../.." && pwd)
cargo build --release --target wasm32-unknown-unknown -p goofi-graphics-web --manifest-path "$root/Cargo.toml"
wasm-bindgen --target web --no-typescript --out-dir "$root/frontend/static/dev/graphics" \
    "$root/target/wasm32-unknown-unknown/release/goofi_graphics_web.wasm"
