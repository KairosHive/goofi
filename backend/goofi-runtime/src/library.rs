//! The one reader of node files. A file of any language is read into the same thing: the engine
//! its node declares, its declaration in the probe schema, and what runs it. An engine registers
//! from that and reads no file of its own.
//!
//! A node declares its engine in its own language: a Python class by the base it derives
//! (`goofi.SignalNode`, `goofi.GraphicsNode`), a Rust file by the SDK whose `export!` it spells,
//! and a `.wgsl` file is a graphics node.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use goofi_core::probe::Introspection;
use goofi_core::node::{Body, NodeFile, Read, Stamp};
use goofi_supervisor::sync::Mutex;

mod probe;
mod rust;
mod shader;

pub use probe::Python;

/// What a read failed with: why, and the engine the file still names where one could be told.
pub(super) type Failed = (String, Option<String>);

pub struct Library {
    python: Option<Python>,
    /// The executable whose `host` mode describes and runs a Rust node built after boot.
    host: Option<PathBuf>,
    /// Set once the boot scan is over: a Rust node read after that was authored in the session.
    booted: AtomicBool,
    /// What every Python probe decided, by the file's bytes and its interpreters: a read under
    /// the graph lock finds what [`Library::prepare`] answered off it.
    probed: Mutex<HashMap<String, Result<Arc<Introspection>, Failed>>>,
    /// What each library a child describes said, by its artifact: a path is a key over its bytes.
    described: Mutex<HashMap<PathBuf, String>>,
}

impl Library {
    pub fn new(python: Option<Python>, host: Option<PathBuf>) -> Library {
        Library { python, host, booted: AtomicBool::new(false), probed: Default::default(), described: Default::default() }
    }

    /// The boot scan is over: a Rust node built from now on runs in a child.
    pub fn boot_done(&self) {
        self.booted.store(true, Ordering::Release);
    }

    /// Everything a read of `dirs` would wait on, done for the caller who asked and off every
    /// lock: each `.rs` file is built, each `.py` file probed. Idempotent.
    pub fn prepare(&self, dirs: &[PathBuf]) {
        let files: Vec<PathBuf> = dirs.iter().filter(|d| d.is_dir()).flat_map(|d| goofi_core::node::node_files(d)).map(|(path, ..)| path).collect();
        for path in files.iter().filter(|p| p.extension().is_some_and(|e| e == "rs")) {
            rust::build(path);
        }
        let python: Vec<&PathBuf> = files.iter().filter(|p| p.extension().is_some_and(|e| e == "py")).collect();
        let width = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8);
        for chunk in python.chunks(width) {
            std::thread::scope(|s| {
                for path in chunk {
                    s.spawn(move || self.probe(path));
                }
            });
        }
    }

    /// Every node file of `dir`, read. It never builds: an `.rs` file [`Library::prepare`] did
    /// not reach says so. A `.py` file it did not reach is probed here, on the caller's thread.
    pub fn read(&self, dir: &Path) -> Vec<NodeFile> {
        let files = goofi_core::node::node_files(dir);
        goofi_supervisor::progress::scanning(dir, files.len());
        files
            .into_iter()
            .map(|(path, type_name, stamp)| {
                let file = self.read_file(dir, path, type_name, stamp);
                goofi_supervisor::progress::scanned(dir, &file.path);
                file
            })
            .collect()
    }

    /// One file, read; none for a file that names no node type.
    pub fn read_one(&self, path: &Path) -> Option<NodeFile> {
        let stamp = std::fs::metadata(path).ok().and_then(|m| Some((m.len(), m.modified().ok()?)));
        Some(self.read_file(path.parent()?, path.to_path_buf(), goofi_core::node::type_name_of(path)?, stamp))
    }

    /// The file in `dir` that names `type_name` for `engine`: the ONE resolver a save, a pack
    /// and `library get` share.
    pub fn find(&self, dir: &Path, type_name: &str, engine: &str) -> Option<PathBuf> {
        let named = goofi_core::node::node_files(dir).into_iter().filter(|(_, name, _)| name == type_name);
        named.map(|(path, name, stamp)| self.read_file(dir, path, name, stamp)).find(|file| file.engine == engine).map(|file| file.path)
    }

    fn read_file(&self, dir: &Path, path: PathBuf, type_name: String, stamp: Option<Stamp>) -> NodeFile {
        let read = match path.extension().and_then(|e| e.to_str()) {
            Some("wgsl") => shader::read(&path),
            Some("py") => self.python(&path),
            _ => rust::read(&path, self.host.as_deref(), self.booted.load(Ordering::Acquire), &self.described),
        };
        let (engine, read) = match read {
            Ok(read) => (read.intro.engine.clone(), Ok(read)),
            // A file that names no engine is its folder's, and the signal engine's elsewhere.
            Err((reason, engine)) => {
                (engine.unwrap_or_else(|| goofi_core::node::engine_of_folder(dir).unwrap_or("signal").to_string()), Err(reason))
            }
        };
        NodeFile { path, type_name, stamp, engine, read }
    }

    fn python(&self, path: &Path) -> Result<Read, Failed> {
        let intro = self.probe(path)?;
        let source = std::fs::read_to_string(path).map_err(|e| (format!("{}: {e}", path.display()), Some(intro.engine.clone())))?;
        let interpreter = self.python.as_ref().map(|python| python.subproc.clone()).unwrap_or_default();
        Ok(Read { intro, body: Body::Python { source, interpreter } })
    }

    /// What the probe of `path` decided, asked once per content and set of interpreters.
    fn probe(&self, path: &Path) -> Result<Arc<Introspection>, Failed> {
        let Some(python) = &self.python else {
            return Err(("no Python interpreter provisioned — run `cargo run -p goofi-init`".into(), None));
        };
        let Some(key) = probe::probe_key(path, &python.interpreters()) else {
            return Err((format!("{} cannot be read", path.display()), None));
        };
        if let Some(known) = self.probed.lock().get(&key) {
            return known.clone();
        }
        let fresh = python.probe(path).map(Arc::new);
        self.probed.lock().insert(key, fresh.clone());
        fresh
    }
}
