//! The manager side of both Python node tiers, behind one `Node` trait.

/// The subprocess tier: `RemoteNode` and the iceoryx2 round-trip to a child interpreter.
pub mod subproc;

/// The in-process tier: `PyNode`, the param-expression evaluator, and discovery. Links libpython.
#[cfg(feature = "embed")]
pub mod inproc;

pub mod discover;
pub use discover::{discover_one, probe_key, Discovered, Discovery};

/// Registers the `goofi` module into the inittab, which must happen BEFORE the interpreter
/// initializes — so every Python entry point in this crate attaches through [`attach`].
#[cfg(feature = "embed")]
mod pyinit {
    use std::sync::Once;

    use goofi_pymod::goofi as goofi_module;
    use pyo3::prelude::*;

    fn ensure_goofi_module() {
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            pyo3::append_to_inittab!(goofi_module);
            // The first import of `signal` claims a SIGINT the C runtime still has at SIG_DFL.
            // On Windows that is always so, and the runtime's console handler would then swallow
            // Ctrl+C before goofi's. Import it once here, on the thread that initializes Python,
            // and give SIGINT back.
            #[cfg(windows)]
            Python::attach(|py| {
                let _ = py.run(c"import signal\nsignal.signal(signal.SIGINT, signal.SIG_DFL)", None, None);
            });
        });
    }

    /// `Python::attach` preceded by the one-time inittab registration.
    pub(crate) fn attach<F, R>(f: F) -> R
    where
        F: for<'py> FnOnce(Python<'py>) -> R,
    {
        ensure_goofi_module();
        Python::attach(f)
    }
}

#[cfg(feature = "embed")]
pub(crate) use pyinit::attach;

pub mod catalog;
