//! Shared discovery and isolation routing for host nodes in every engine.
use crate::{Discovered, Discovery};
use std::path::Path;
/// The interpreters the scan probes and runs with. The subprocess one is the caller's; the
/// free-threaded one is the one this build links, if any, and it is what routes a file in-process.
#[derive(Clone, Debug)]
pub struct Python {
    pub subproc: String,
    pub free_threaded: Option<String>,
    /// Where a probe's answer outlives the process: in the build dir, beside the built nodes.
    pub memo: std::path::PathBuf,
}

impl Python {
    pub fn new(subproc: String) -> Python {
        let memo = goofi_build::base_dir(&goofi_core::home::dir()).join("probes");
        Python { subproc, free_threaded: free_threaded(), memo }
    }

    pub fn interpreters(&self) -> Vec<&str> {
        std::iter::once(self.subproc.as_str()).chain(self.free_threaded.as_deref()).collect()
    }
}

/// What one file's probes decided, before anything is registered.
#[derive(Clone)]
pub enum Probed {
    InProcess(Discovered),
    Subprocess(Discovered),
    Unavailable(String),
}

impl Probed {
    /// The same answer, against the file at `path`. A probe's answer depends on the BYTES, so it
    /// is memoised across paths with the same type name — the source is read from there
    /// at registration.
    pub fn at(self, path: &Path) -> Probed {
        let moved = |d: Discovered| Discovered { source: path.to_path_buf(), ..d };
        match self {
            Probed::InProcess(d) => Probed::InProcess(moved(d)),
            Probed::Subprocess(d) => Probed::Subprocess(moved(d)),
            Probed::Unavailable(reason) => Probed::Unavailable(reason),
        }
    }
}

/// The GIL-gate router: a file whose imports keep the GIL disabled runs in-process, any other in
/// a subprocess. One interpreter per probe, because re-enabling the GIL is one-way.
pub fn probe(path: &Path, python: Option<&Python>) -> Probed {
    let Some(python) = python else {
        return Probed::Unavailable("no Python interpreter provisioned — run `cargo run -p goofi-init`".into());
    };
    if let Some(ft) = python.free_threaded.as_deref() {
        if let Discovery::Found(d) = in_process(path, ft, &python.memo) {
            if d.gil_safe {
                return Probed::InProcess(d);
            }
            // A re-enabled GIL falls through to the subprocess tier, as a failed probe does.
        }
    }
    match crate::subproc::probe(path, &python.subproc, &python.memo) {
        Discovery::Found(d) => Probed::Subprocess(d),
        Discovery::Unavailable { reason, .. } => Probed::Unavailable(reason),
        Discovery::Skip => Probed::Unavailable("not a node file".into()),
    }
}

#[cfg(feature = "embed")]
fn in_process(path: &Path, ft: &str, memo: &Path) -> Discovery {
    crate::inproc::probe(path, ft, memo)
}

#[cfg(feature = "embed")]
fn free_threaded() -> Option<String> {
    crate::inproc::interpreter_path()
}

#[cfg(not(feature = "embed"))]
fn in_process(_path: &Path, _ft: &str, _memo: &Path) -> Discovery {
    Discovery::Skip
}

#[cfg(not(feature = "embed"))]
fn free_threaded() -> Option<String> {
    None
}

/// A discovered in-process type, registered ROUTED: its tier cell decides the tier at every
/// build, so the runtime GIL tripwire demoting it is all a re-route takes.
#[cfg(feature = "embed")]
pub fn routed(
    d: Discovered,
    subproc: &str,
) -> (&'static goofi_node::NodeManifest, goofi_host_sdk::NodeFactory, &'static goofi_node::IsolationCell) {
    let t = crate::routed_node_type(d, subproc);
    (t.manifest, t.factory, t.isolation)
}

#[cfg(not(feature = "embed"))]
pub fn routed(
    d: Discovered,
    subproc: &str,
) -> (&'static goofi_node::NodeManifest, goofi_host_sdk::NodeFactory, &'static goofi_node::IsolationCell) {
    let t = crate::subproc::node_type_from(subproc, d);
    (t.manifest, t.factory, t.isolation)
}
