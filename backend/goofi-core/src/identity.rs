//! Session identity survives a clone or rename and changes when its owner is replaced.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity(u64);

impl Default for Identity {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

impl Identity {
    /// The resource generation used in a derived transport address.
    pub fn generation(&self) -> u64 { self.0 }
}
