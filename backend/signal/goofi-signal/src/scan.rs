//! The signal engine's scan of one `nodes_signal/` folder: a `.py` file is probed in the
//! interpreter that will run it and registered on the tier its imports allow; an `.rs` file is
//! built through goofi-build — or found built — and loaded behind its version symbol.

use std::path::Path;
use std::sync::Arc;

use goofi_node::{Isolation, Scanned, ScannedType};
use goofi_signal_sdk::host::Loaded;

use crate::SignalEngine;

pub use goofi_python::catalog::Python;
use goofi_python::catalog::{Probed, probe, routed};

impl SignalEngine {
    pub fn set_python(&mut self, python: Python) {
        self.python = Some(python);
    }
}

pub(crate) fn scan(engine: &mut SignalEngine, dir: &Path) -> Vec<ScannedType> {
    let (rust, paths): (Vec<_>, Vec<_>) =
        goofi_node::node_files(dir, goofi_node::Engine::id(engine)).into_iter().partition(|(p, _, _)| p.extension().is_some_and(|e| e == "rs"));
    // Probes spawn an interpreter each, so a folder is probed a few files at a time; a file whose
    // key — its type name, bytes and interpreter environment — is already decided is not probed again.
    let width = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8);
    let mut probes: Vec<Option<Probed>> = Vec::with_capacity(paths.len());
    for chunk in paths.chunks(width) {
        let python = engine.python.clone();
        let keyed: Vec<Option<String>> = chunk
            .iter()
            .map(|(p, name, _)| {
                python.as_ref().and_then(|py| goofi_python::probe_key(p, &py.interpreters()))
                    .map(|key| format!("{name}:{key}"))
            })
            .collect();
        let cached: Vec<Option<Probed>> =
            keyed.iter().map(|k| k.as_ref().and_then(|k| engine.probed.get(k).cloned())).collect();
        let decided: Vec<Option<Probed>> = std::thread::scope(|s| {
            let handles: Vec<_> = chunk
                .iter()
                .zip(&cached)
                .map(|((p, _, _), hit)| {
                    let python = python.as_ref();
                    s.spawn(move || hit.clone().unwrap_or_else(|| probe(p, python)).at(p))
                })
                .collect();
            handles.into_iter().zip(chunk).map(|(h, (p, _, _))| {
                let decided = h.join().ok();
                goofi_core::startup::scanned(dir, p);
                decided
            }).collect()
        });
        for (key, probed) in keyed.into_iter().zip(decided) {
            if let (Some(key), Some(probed)) = (key, &probed) {
                engine.probed.insert(key, probed.clone());
            }
            probes.push(probed);
        }
    }
    let mut out = Vec::new();
    for ((_, type_name, stamp), probed) in paths.into_iter().zip(probes) {
        let Some(probed) = probed else { continue };
        let outcome = engine.register(&type_name, probed);
        out.push(ScannedType { type_name, stamp, outcome });
    }
    for (path, type_name, stamp) in rust {
        let outcome = engine.register_rust(&path, &type_name);
        goofi_core::startup::scanned(dir, &path);
        out.push(ScannedType { type_name, stamp, outcome });
    }
    out
}

impl SignalEngine {
    /// An `.rs` file: the artifact the prebuild left for these bytes, loaded — a library that will
    /// not load displaces a stale registration and greys the type out with the reason.
    fn register_rust(&mut self, path: &Path, type_name: &str) -> Scanned {
        let base = goofi_build::base_dir(&goofi_core::home::dir());
        let hosted = self.booted;
        let loaded = goofi_build::built(&goofi_build::SIGNAL, path, &base).and_then(|artifact| {
            if hosted { self.host_rust(&artifact, type_name) } else { self.load_rust(&artifact, type_name) }
        });
        match loaded {
            Ok(replaced) => Scanned::Registered { isolation: if hosted { Isolation::Hosted } else { Isolation::Native }, replaced },
            Err(reason) => {
                self.remove_dyn_type(type_name);
                Scanned::Unavailable(reason)
            }
        }
    }

    /// After boot: the library is described by a child and run by one, so this process never
    /// loads it — a re-authored node's newest build is what runs.
    fn host_rust(&mut self, artifact: &Path, type_name: &str) -> Result<bool, String> {
        let host = self.host.clone().ok_or("no host executable was named, so a node built after boot cannot run")?;
        let intro = goofi_node::parse_introspection(&crate::hosted::describe(&host, artifact)?)?;
        if let Some(reason) = goofi_node::illegal_slot(&intro).or_else(|| goofi_node::foreign_output(&intro, None)) {
            return Err(reason);
        }
        let manifest = goofi_node::leak_manifest(type_name.to_string(), &intro)?;
        let artifact = artifact.to_path_buf();
        let factory: goofi_signal_sdk::NodeFactory =
            Box::new(move |_| Box::new(crate::hosted::HostedNode::new(host.clone(), artifact.clone(), manifest)));
        Ok(self.register_dyn_type(manifest, factory, &goofi_node::HOSTED))
    }

    fn load_rust(&mut self, artifact: &Path, type_name: &str) -> Result<bool, String> {
        if !self.rust_loaded.contains_key(artifact) {
            let opened = goofi_build::open(artifact)?;
            let intro = goofi_node::parse_introspection(&opened.describe)?;
            if let Some(reason) = goofi_node::illegal_slot(&intro).or_else(|| goofi_node::foreign_output(&intro, None)) {
                return Err(reason);
            }
            let manifest = goofi_node::leak_manifest(type_name.to_string(), &intro)?;
            let loaded = unsafe { Loaded::open(opened.library, manifest) }?;
            self.rust_loaded.insert(artifact.to_path_buf(), Arc::new(loaded));
        }
        let loaded = self.rust_loaded[artifact].clone();
        let manifest = loaded.manifest();
        let factory: goofi_signal_sdk::NodeFactory = Box::new(move |_| loaded.instantiate());
        Ok(self.register_dyn_type(manifest, factory, &goofi_node::NATIVE))
    }
}

impl SignalEngine {
    fn register(&mut self, type_name: &str, probed: Probed) -> Scanned {
        let subproc = self.python.as_ref().map(|p| p.subproc.clone()).unwrap_or_default();
        match probed {
            Probed::InProcess(d) => {
                let (manifest, factory, tier) = routed(d, &subproc);
                let isolation = tier.get();
                Scanned::Registered { isolation, replaced: self.register_dyn_type(manifest, factory, tier) }
            }
            Probed::Subprocess(d) => {
                let t = goofi_python::subproc::node_type_from(&subproc, d);
                let isolation = t.isolation.get();
                Scanned::Registered {
                    isolation,
                    replaced: self.register_dyn_type(t.manifest, t.factory, t.isolation),
                }
            }
            // The latest scan is the answer: a stale runtime type is displaced first.
            Probed::Unavailable(reason) => {
                self.remove_dyn_type(type_name);
                Scanned::Unavailable(reason)
            }
        }
    }
}

