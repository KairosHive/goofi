//! The signal engine and its shared host runtime.
mod engine;
pub mod scan;
pub use engine::SignalEngine;
pub use scan::Python;
impl SignalEngine {
    pub fn of(engine: &mut dyn goofi_node::Engine) -> Option<&mut SignalEngine> {
        engine.as_any_mut().downcast_mut()
    }
}
