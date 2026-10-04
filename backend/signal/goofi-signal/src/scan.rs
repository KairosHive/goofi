//! The signal engine's scan of one `nodes_signal/` folder: a `.py` file is probed in the
//! interpreter that will run it and registered on the tier its imports allow; an `.rs` file is
//! built through goofi-build — or found built — and loaded behind its version symbol.

use std::path::{Path, PathBuf};

use goofi_node::{Engine, Isolation, Scanned, ScannedType};
use crate::SignalEngine;

pub use goofi_python::catalog::Python;
use goofi_python::catalog::{Probed, probed, routed, warm};

impl SignalEngine {
    pub fn set_python(&mut self, python: Python) {
        self.python = Some(python);
    }

    /// The probes a scan of `dir` would spawn, as work for off the lock.
    pub(crate) fn prepare(&self, dir: &Path) -> Option<Box<dyn FnOnce() + Send>> {
        let python = self.python.clone()?;
        let files: Vec<(std::path::PathBuf, String)> = goofi_node::node_files(dir, goofi_node::Engine::id(self))
            .into_iter()
            .filter(|(p, _, _)| p.extension().is_none_or(|e| e != "rs"))
            .map(|(p, name, _)| (p, name))
            .collect();
        (!files.is_empty()).then(|| Box::new(move || warm(&files, &python)) as Box<dyn FnOnce() + Send>)
    }
}

pub(crate) fn scan(engine: &mut SignalEngine, dir: &Path) -> Vec<ScannedType> {
    let (rust, paths): (Vec<_>, Vec<_>) =
        goofi_node::node_files(dir, goofi_node::Engine::id(engine)).into_iter().partition(|(p, _, _)| p.extension().is_some_and(|e| e == "rs"));
    // A probe spawns an interpreter, so `prepare` ran them off the lock; here each is a lookup,
    // and only a file that pass never saw is probed now.
    let mut out = Vec::new();
    for (path, type_name, stamp) in paths {
        let probed = probed(&path, &type_name, engine.python.as_ref());
        goofi_supervisor::progress::scanned(dir, &path);
        let Some(probed) = probed else { continue };
        let outcome = engine.register(&type_name, probed);
        out.push(ScannedType { type_name, stamp, outcome });
    }
    for (path, type_name, stamp) in rust {
        let outcome = engine.register_rust(&path, &type_name);
        goofi_supervisor::progress::scanned(dir, &path);
        out.push(ScannedType { type_name, stamp, outcome });
    }
    out
}

impl SignalEngine {
    /// An `.rs` file: the artifact the prebuild left for these bytes, loaded — a library that will
    /// not load displaces a stale registration and greys the type out with the reason.
    fn register_rust(&mut self, path: &Path, type_name: &str) -> Scanned {
        let base = goofi_supervisor::layout::runtime().build();
        let hosted = self.booted;
        let loaded = goofi_build::built(&goofi_build::SIGNAL, path, &base).and_then(|artifact| self.load_rust(artifact, type_name));
        match loaded {
            Ok(replaced) => Scanned::Registered { isolation: if hosted { Isolation::Hosted } else { Isolation::Native }, replaced },
            Err(reason) => {
                self.remove_type(type_name);
                Scanned::Unavailable(reason)
            }
        }
    }

    /// After boot the library is described and run by a child, so this process never loads it
    /// and a re-authored node's newest build is what runs.
    fn load_rust(&mut self, artifact: PathBuf, type_name: &str) -> Result<bool, String> {
        let (manifest, factory, tier) = goofi_runtime::hosted::load(
            self.iox.clone(), artifact, type_name, None, self.host.as_deref(), self.booted,
        )?;
        Ok(self.register_dyn_type(manifest, factory, tier))
    }
}

impl SignalEngine {
    fn register(&mut self, type_name: &str, probed: Probed) -> Scanned {
        let subproc = self.python.as_ref().map(|p| p.subproc.clone()).unwrap_or_default();
        match probed {
            Probed::InProcess(d) | Probed::Subprocess(d) => {
                let (manifest, factory, tier) = routed(self.iox.clone(), d, &subproc);
                Scanned::Registered { isolation: tier.get(), replaced: self.register_dyn_type(manifest, factory, tier) }
            }
            // The latest scan is the answer: a stale runtime type is displaced first.
            Probed::Unavailable(reason) => {
                self.remove_type(type_name);
                Scanned::Unavailable(reason)
            }
        }
    }
}
