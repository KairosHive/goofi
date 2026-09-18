//! The graphics engine: shader nodes on one GPU device, planned and drawn from one code base on
//! two hosts. The process runs it behind the `Engine` seam; a page runs it on WebGPU.
// A page has no readback reader yet: the rings, taps and the adapter name wait for Phase 2.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

mod gpu;
mod host;
mod pipeline;
mod resources;
mod shader;

#[cfg(not(target_arch = "wasm32"))]
mod engine;
#[cfg(not(target_arch = "wasm32"))]
mod half;
#[cfg(not(target_arch = "wasm32"))]
mod plan;
#[cfg(not(target_arch = "wasm32"))]
mod producer;
#[cfg(not(target_arch = "wasm32"))]
mod runtime;
#[cfg(not(target_arch = "wasm32"))]
mod scan;
#[cfg(target_arch = "wasm32")]
pub mod web;

pub use host::Host;
#[cfg(not(target_arch = "wasm32"))]
pub use engine::{Clock, GraphicsEngine, GraphicsStatus, FPS};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use engine::Instance;
