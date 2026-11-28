//! The signal engine and its shared host runtime.
mod engine;
mod scan;
pub use engine::SignalEngine;
pub use goofi_runtime::python::catalog::free_threaded;
impl SignalEngine {
    pub fn of(engine: &mut dyn goofi_core::node::Engine) -> Option<&mut SignalEngine> {
        engine.as_any_mut().downcast_mut()
    }
}
