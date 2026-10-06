//! The signal engine behind the seam: one runtime per node on the shared per-node runtime, told
//! its whole desired state at every settle, and the library of the async tiers.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use goofi_node::{DrainWaker, Engine, GraphView, IsolationCell, LibraryEntry, NodeManifest, ParamGroups, Status, Touched, Uid};
use goofi_runtime::{Handle, HostExecutor, Plane, Shared};

/// A [`goofi_signal_sdk::NodeFactory`] shared with the node's own thread, which is where the build happens.
type SharedFactory = Arc<dyn Fn(&ParamGroups) -> Box<dyn goofi_signal_sdk::Node> + Send + Sync>;

/// A registered type: its manifest, its tier cell and how an instance is built.
struct DynType {
    manifest: &'static NodeManifest,
    isolation: &'static IsolationCell,
    factory: SharedFactory,
}

pub struct SignalEngine {
    /// What every port of this engine and its nodes is built against.
    pub(crate) iox: Arc<goofi_transport::Iox>,
    /// What service names are scoped by — handed down from the graph, whose resolver inputs it is.
    instance: String,
    time: Arc<goofi_core::time::Time>,
    shared: Arc<Shared>,
    hosts: HashMap<Uid, Handle>,
    /// A node was born since the last settle, and owes its runtime a first desired state.
    dirty: bool,
    dyn_types: HashMap<&'static str, DynType>,
    /// The interpreters a `.py` file is probed and run with; none until the host provides them.
    pub(crate) python: Option<crate::scan::Python>,
    /// Set once the boot scan is over: a Rust node registered after that runs hosted.
    pub(crate) booted: bool,
    /// The executable that hosts a node built after boot — goofi's own binary, or the harness's.
    pub(crate) host: Option<std::path::PathBuf>,
    /// The engine's own iceoryx2 node, which every runtime's bell is opened on. Declared LAST: it
    /// must drop after every port built from it, or it leaves its own directory behind.
    bells: Option<goofi_transport::IoxNode>,
}

impl SignalEngine {
    /// Name the executable whose `host` mode runs a node built after boot.
    pub fn set_host(&mut self, exe: std::path::PathBuf) {
        self.host = Some(exe);
    }

    pub fn new(iox: Arc<goofi_transport::Iox>, instance: String, time: Arc<goofi_core::time::Time>, waker: Arc<DrainWaker>) -> Result<SignalEngine, String> {
        Ok(SignalEngine {
            shared: Arc::new(Shared::new(waker)),
            iox,
            instance,
            time,
            hosts: HashMap::new(),
            dirty: false,
            dyn_types: HashMap::new(),
            python: None,
            booted: false,
            host: None,
            bells: None,
        })
    }

    /// Register a type; `manifest` leaks, once per type. A name another type held is REPLACED,
    /// because a rescan re-registers what it finds — answered as `true`.
    pub fn register_dyn_type(
        &mut self,
        manifest: &'static NodeManifest,
        factory: goofi_signal_sdk::NodeFactory,
        isolation: &'static IsolationCell,
    ) -> bool {
        let name = manifest.type_name;
        self.dyn_types.insert(name, DynType { manifest, isolation, factory: Arc::from(factory) }).is_some()
    }

    /// The params as this engine counts them: the author's, then the universal `common` group.
    fn decls_of(manifest: &'static NodeManifest) -> Vec<goofi_node::ParamDecl> {
        manifest.params.iter().copied().chain(goofi_runtime::common_decls(manifest)).collect()
    }
}

impl Engine for SignalEngine {
    fn id(&self) -> &'static str {
        "signal"
    }

    fn dirty(&self) -> bool {
        self.dirty || self.shared.replan.load(Ordering::Acquire)
    }

    fn boot_done(&mut self) {
        self.booted = true;
    }

    fn prepare(&self, dir: &std::path::Path) -> Option<Box<dyn FnOnce() + Send>> {
        self.prepare(dir)
    }

    fn scan(&mut self, dir: &std::path::Path) -> Vec<goofi_node::ScannedType> {
        crate::scan::scan(self, dir)
    }

    fn remove_type(&mut self, type_name: &str) -> bool {
        self.dyn_types.remove(type_name).is_some()
    }

    fn rust_sdk(&self) -> Option<&'static str> {
        Some(goofi_build::SIGNAL.name)
    }

    fn library(&self) -> Vec<LibraryEntry> {
        self.dyn_types
            .values()
            .map(|dt| LibraryEntry { manifest: dt.manifest, isolation: dt.isolation })
            .collect()
    }

    fn insert(&mut self, uid: Uid, type_name: &str, generation: u64, params: &ParamGroups) -> Option<String> {
        let Some(dt) = self.dyn_types.get(type_name) else {
            return Some(format!("no node type `{type_name}` in the signal library"));
        };
        let (manifest, factory) = (dt.manifest, dt.factory.clone());
        if self.bells.is_none() {
            match self.iox.node() {
                Ok(node) => self.bells = Some(node),
                Err(e) => return Some(e),
            }
        }
        let decls = Self::decls_of(manifest);
        let atomics: Arc<[AtomicU64]> = goofi_runtime::cells_of(params, &decls).into();
        let spawn = goofi_runtime::Spawn {
            engine: "signal",
            uid,
            instance: self.instance.clone(),
            base: goofi_transport::service_base(&self.instance, uid, generation),
            manifest,
            decls: decls.clone(),
            params: atomics,
            time: self.time.clone(),
        };
        let params = params.clone();
        let make = move || {
            let build: goofi_runtime::NodeBuild = Box::new(move |p| factory(p));
            HostExecutor::new(manifest, decls, build, &params)
        };
        match goofi_runtime::spawn(&self.iox, spawn, self.shared.clone(), self.bells.as_ref().expect("opened above"), make) {
            Ok(handle) => {
                self.hosts.insert(uid, handle);
                self.dirty = true;
                None
            }
            Err(e) => Some(e),
        }
    }

    fn remove(&mut self, uid: Uid) {
        // Dropping the handle halts the thread without waiting: it may be inside a long
        // `process()`, and every caller holds the graph mutex.
        self.hosts.remove(&uid);
        self.shared.reports.lock().retain(|(u, _)| *u != uid);
    }

    fn settle(&mut self, view: &GraphView<'_>, _touched: &[Touched]) {
        self.dirty = false;
        self.shared.replan.store(false, Ordering::Release);
        let plane = Plane { kind: None, planned: &|_| false, records: true };
        for (uid, handle) in &self.hosts {
            let Some(nv) = view.nodes.get(uid) else { continue };
            let decls = Self::decls_of(nv.manifest);
            handle.send_if_changed(goofi_runtime::desired_of(view, *uid, nv, &decls, &plane));
        }
    }

    fn drain(&mut self, apply: &mut dyn FnMut(Uid, Status)) -> usize {
        self.shared.drain(&mut Vec::new(), apply)
    }

    fn request(&mut self, uid: Uid, request: goofi_node::Request) {
        if let Some(handle) = self.hosts.get(&uid) {
            handle.request(request);
        }
    }

    /// Every node born after computes from the new origin.
    fn set_evaluator(&mut self, evaluator: Arc<dyn goofi_node::ExprEvaluator>) {
        *self.shared.evaluator.lock() = Some(evaluator);
    }

    /// The `common` scheduling group: signal semantics, added to every signal node.
    fn universal_decls(&self, manifest: &'static NodeManifest) -> Vec<goofi_node::ParamDecl> {
        goofi_runtime::common_decls(manifest).to_vec()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    /// Stop every node and WAIT for each to release its shared memory — a ceiling, because only a
    /// process about to EXIT has no "a moment later".
    fn shutdown(&mut self) {
        goofi_runtime::stop_all(self.hosts.values());
        self.hosts.clear();
    }
}
