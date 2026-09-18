//! What the engine takes from the platform it runs on, and nothing else. The process is one
//! host; a page is the other. Everything that plans, allocates and draws is host-blind.

/// The platform under the engine.
pub trait Host: Send + Sync {
    /// Patch seconds now: what the `time` uniform of every stage drawn this tick reads.
    fn now(&self) -> f64;
}

#[cfg(not(target_arch = "wasm32"))]
impl Host for goofi_core::time::Time {
    fn now(&self) -> f64 {
        goofi_core::time::Time::now(self)
    }
}
