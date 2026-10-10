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

    /// A thread's Python state, made at its first attach and kept until the thread ends. Without
    /// one held, every attach would make a state and tear it down again, heap purge included.
    struct Held {
        gil: pyo3::ffi::PyGILState_STATE,
        tstate: *mut pyo3::ffi::PyThreadState,
    }

    impl Drop for Held {
        fn drop(&mut self) {
            // SAFETY: the state was made by this thread and is detached; an interpreter that is
            // gone or going owns nothing to give back.
            unsafe {
                if pyo3::ffi::Py_IsInitialized() != 0 && pyo3::ffi::Py_IsFinalizing() == 0 {
                    pyo3::ffi::PyEval_RestoreThread(self.tstate);
                    pyo3::ffi::PyGILState_Release(self.gil);
                }
            }
        }
    }

    thread_local! {
        static HELD: std::cell::OnceCell<Held> = const { std::cell::OnceCell::new() };
    }

    /// `Python::attach` preceded by the one-time inittab registration, on a thread that holds
    /// its Python state.
    pub(crate) fn attach<F, R>(f: F) -> R
    where
        F: for<'py> FnOnce(Python<'py>) -> R,
    {
        ensure_goofi_module();
        HELD.with(|held| {
            held.get_or_init(|| {
                Python::initialize();
                // SAFETY: the interpreter is up; ensure makes this thread's state and save
                // detaches it, so the thread holds a state without blocking the interpreter.
                unsafe {
                    let gil = pyo3::ffi::PyGILState_Ensure();
                    let tstate = pyo3::ffi::PyEval_SaveThread();
                    Held { gil, tstate }
                }
            });
        });
        Python::attach(f)
    }
}

#[cfg(feature = "embed")]
pub(crate) use pyinit::attach;

pub mod catalog;
