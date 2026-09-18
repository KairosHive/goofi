//! The engine's scan of one `nodes_graphics/` folder, and the thread that builds a pipeline for
//! what it registered. A driver takes its time over a compile, so no op waits on one: the class
//! holds a cell, and the plan picks it up at the tick after it is filled.

use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, OnceLock};

use goofi_node::{NodeManifest, Scanned, ScannedType};

pub use crate::pipeline::Built;
use crate::pipeline::{compile, Job};
use crate::GraphicsEngine;

/// One graphics source as the engine holds it.
pub struct Class {
    pub manifest: &'static NodeManifest,
    pub feedback: bool,
    pub window: bool,
    /// The buffers this type carries between two ticks, in the order the prelude binds them.
    pub state: Vec<String>,
    pub kind: Kind,
    pub isolation: &'static goofi_node::IsolationCell,
}

pub enum Kind {
    Shader(Built),
    Host(crate::producer::Factory),
}

pub(crate) fn scan(engine: &mut GraphicsEngine, dir: &Path) -> Vec<ScannedType> {
    let mut out = Vec::new();
    for (path, type_name, stamp) in goofi_node::node_files(dir, "graphics") {
        // A file with no stamp to compare is read again: unreadable metadata proves nothing.
        let seen = stamp.map(|s| (path.clone(), s));
        let unchanged = seen.is_some() && engine.stamps.get(&type_name) == seen.as_ref() && engine.classes.contains_key(&type_name);
        let registered = if unchanged { Ok(false) } else { engine.register(&path, &type_name) };
        let outcome = match registered {
            Ok(replaced) => {
                match seen {
                    Some(s) => engine.stamps.insert(type_name.clone(), s),
                    None => engine.stamps.remove(&type_name),
                };
                Scanned::Registered { isolation: engine.classes[&type_name].isolation.get(), replaced }
            }
            Err(reason) => {
                // A file that no longer loads displaces its registration, so the palette greys the
                // type rather than offering one nothing can build.
                crate::gpu::give_back(engine.classes.remove(type_name.as_str()));
                engine.stamps.remove(&type_name);
                Scanned::Unavailable(reason)
            }
        };
        goofi_core::startup::scanned(dir, &path);
        out.push(ScannedType { type_name, stamp, outcome });
    }
    out
}

impl GraphicsEngine {
    /// One file: its header is the manifest, its text plus the prelude is what naga judges, and
    /// only then does a pipeline get asked for.
    pub(crate) fn register(&mut self, path: &Path, type_name: &str) -> Result<bool, String> {
        if path.extension().is_some_and(|ext| ext != "wgsl") {
            return self.register_host(path, type_name);
        }
        let source = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (intro, manifest, job) = Job::shader(type_name, &source)?;
        let pipeline = self.compiler.build(job);
        let class =
            Arc::new(Class { manifest, feedback: intro.feedback, window: intro.window, state: intro.state, kind: Kind::Shader(pipeline), isolation: &goofi_node::SHADER });
        let displaced = self.classes.insert(type_name.to_string(), class);
        let replaced = displaced.is_some();
        crate::gpu::give_back(displaced);
        Ok(replaced)
    }
}

impl Compiler {
    pub fn program(&self, manifest: &'static NodeManifest, source: &str) -> Result<Built, String> {
        Ok(self.build(Job::program(manifest, source)?))
    }
}

/// One job on the compile thread's queue: what to build, and where to leave it.
struct Order {
    job: Job,
    cell: Built,
    shared: Arc<goofi_control::Shared>,
}

/// Where every pipeline in the process is built: ONE thread, beside the one device, so no op
/// waits on a compile.
fn compiler() -> Option<&'static mpsc::Sender<Order>> {
    static ONE: OnceLock<Option<mpsc::Sender<Order>>> = OnceLock::new();
    ONE.get_or_init(|| {
        let gpu = crate::gpu::shared().ok()?;
        let (jobs, take) = mpsc::channel::<Order>();
        goofi_core::worker::thread("goofi-graphics-compile")
            .spawn(move || {
                while let Ok(order) = take.recv() {
                    let _ = order.cell.set(pollster::block_on(compile(&gpu, &order.job)));
                    // The tick picks the cell up by itself; the settle is for a refusal, which
                    // only a plan can turn into the node's standing error.
                    order.shared.ask_settle();
                    // The order may hold the last handle on the pipeline it just built.
                    crate::gpu::give_back(order);
                }
            })
            .ok()?;
        Some(jobs)
    })
    .as_ref()
}

/// One engine's end of that queue: the shared state a finished compile must wake.
#[derive(Clone)]
pub struct Compiler(pub Arc<goofi_control::Shared>);

impl Compiler {
    pub fn build(&self, job: Job) -> Built {
        let cell: Built = Arc::new(OnceLock::new());
        let order = Order { job, cell: cell.clone(), shared: self.0.clone() };
        match compiler() {
            Some(jobs) => {
                let _ = jobs.send(order);
            }
            None => {
                let _ = cell.set(Err("no compile thread".into()));
            }
        }
        cell
    }
}
