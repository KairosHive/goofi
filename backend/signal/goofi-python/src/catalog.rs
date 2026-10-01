//! Shared discovery and isolation routing for host nodes in every engine.
use crate::{Discovered, Discovery};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
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
        let memo = goofi_build::base_dir(&goofi_supervisor::home::dir()).join("probes");
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

/// What every probe decided, by the type name, the file's bytes and its interpreters. One per
/// process, so a scan under the graph lock finds what a `warm` pass off it already answered.
static PROBED: LazyLock<Mutex<HashMap<String, Probed>>> = LazyLock::new(Default::default);

fn key_of(path: &Path, name: &str, python: &Python) -> Option<String> {
    crate::probe_key(path, &python.interpreters()).map(|key| format!("{name}:{key}"))
}

/// Probe every file of `files` (its path and type name) not decided yet, a few at a time:
/// each probe spawns an interpreter. Safe to run off every lock, and idempotent.
pub fn warm(files: &[(PathBuf, String)], python: &Python) {
    let width = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8);
    for chunk in files.chunks(width) {
        let keyed: Vec<Option<String>> = chunk.iter().map(|(p, name)| key_of(p, name, python)).collect();
        let missing: Vec<&(PathBuf, String)> = {
            let cache = PROBED.lock().unwrap_or_else(|e| e.into_inner());
            chunk.iter().zip(&keyed).filter(|(_, k)| k.as_ref().is_none_or(|k| !cache.contains_key(k))).map(|(f, _)| f).collect()
        };
        let decided: Vec<Option<Probed>> = std::thread::scope(|s| {
            let handles: Vec<_> = missing.iter().map(|(p, _)| s.spawn(move || probe(p, Some(python)))).collect();
            handles.into_iter().map(|h| h.join().ok()).collect()
        });
        let mut cache = PROBED.lock().unwrap_or_else(|e| e.into_inner());
        for ((p, name), probed) in missing.into_iter().zip(decided) {
            if let (Some(key), Some(probed)) = (key_of(p, name, python), probed) {
                cache.insert(key, probed);
            }
        }
    }
}

/// What the probe of `path` decided, against that path. None where no interpreter is provisioned
/// or the file cannot be read; a file `warm` did not reach is probed here, on the caller's thread.
pub fn probed(path: &Path, name: &str, python: Option<&Python>) -> Option<Probed> {
    let python = python?;
    let key = key_of(path, name, python)?;
    let hit = PROBED.lock().unwrap_or_else(|e| e.into_inner()).get(&key).cloned();
    let probed = hit.unwrap_or_else(|| {
        let fresh = probe(path, Some(python));
        PROBED.lock().unwrap_or_else(|e| e.into_inner()).insert(key, fresh.clone());
        fresh
    });
    Some(probed.at(path))
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

/// A discovered type, registered ROUTED: its tier cell decides the tier at every build, so the
/// runtime GIL tripwire demoting it is all a re-route takes.
pub fn routed(
    iox: std::sync::Arc<goofi_transport::Iox>,
    d: Discovered,
    subproc: &str,
) -> (&'static goofi_node::NodeManifest, goofi_host_sdk::NodeFactory, &'static goofi_node::IsolationCell) {
    let (manifest, tier) = (d.manifest, d.isolation);
    let in_slots = goofi_host_sdk::host::in_slots(manifest);
    #[cfg(feature = "embed")]
    let out_slots: Vec<&'static str> = manifest.outputs.iter().map(|o| o.name).collect();
    let source = std::fs::read_to_string(&d.source).unwrap_or_default();
    let python = subproc.to_string();
    let factory: goofi_host_sdk::NodeFactory = Box::new(move |_p| match tier.get() {
        #[cfg(feature = "embed")]
        goofi_node::Isolation::InProcess => crate::inproc::build_routed(&source, in_slots.clone(), out_slots.clone(), tier),
        _ => Box::new(crate::subproc::RemoteNode::new(crate::subproc::subproc(iox.clone(), &python, &source), in_slots.clone()))
            as Box<dyn goofi_host_sdk::Node>,
    });
    (manifest, factory, tier)
}
