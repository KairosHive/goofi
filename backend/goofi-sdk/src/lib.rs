//! What a Rust node file is written against, one module per engine: the module a file's
//! `export!` names is the engine its node declares. `signal` and `graphics` are one contract,
//! a node the engine calls with frames; `audio` is a node the audio thread calls per block.

pub mod audio;
pub mod node;

/// A node of the signal engine: it runs when a frame arrives, or on its own schedule.
pub mod signal {
    pub use crate::node::*;
}

/// A node the render tick calls at its place in the frame: it makes one picture.
pub mod graphics {
    pub use crate::node::*;
}
