//! The signal engine and its shared host runtime.
mod engine;
pub mod runtime;
pub mod scan;
pub mod hosted;
pub use engine::SignalEngine;
pub use scan::Python;
pub use goofi_host::{RunPolicy, with_common, common_decls, FREQ_MODE_UPDATES_PER_SECOND, FREQ_MODE_SECONDS_PER_UPDATE};
impl SignalEngine {
    pub fn of(engine: &mut dyn goofi_node::Engine) -> Option<&mut SignalEngine> {
        engine.as_any_mut().downcast_mut()
    }
}
