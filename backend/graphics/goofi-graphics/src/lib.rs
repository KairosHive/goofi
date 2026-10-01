//! The graphics engine behind the `Engine` seam: shader nodes on the GPU, one device, one render
//! thread, no window. The engine (this file) owns the library, the plan and the clock; the render
//! thread (`runtime`) owns the textures and draws; a node's control half (`half`) is a thread of
//! its own, parked on the node's door — the same one the audio engine uses.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use goofi_supervisor::sync::Mutex;
use std::time::{Duration, Instant};

use goofi_runtime::{Desired, Handle, HostExecutor, Plane, Shared};
use goofi_core::SlotType;
use goofi_node::{
    DrainWaker, Engine, ExprDecl, ExprMode, GraphView, LibraryEntry, NodeManifest, NodeView,
    ParamDecl, ParamGroups, ParamSpec, ScannedType, Status, Touched, Uid,
};

mod gpu;
mod half;
mod plan;
mod runtime;
mod scan;
mod shader;
mod producer;
mod resources;

use gpu::Gpu;
use half::GraphicsHalf;
use runtime::{Runtime, Stats};
use scan::{Class, Compiler};

/// The rate the engine renders at under its own clock, which is also the rate a viewer draws at
/// and the rate a recording is encoded at. A tick the clock did not pace — the harness's `render`
/// — is why every frame's instant is written beside the video rather than trusted to the
/// container.
pub const FPS: u32 = 30;

/// The pace that rate asks of the clock thread.
const PERIOD: Duration = Duration::from_nanos(1_000_000_000 / FPS as u64);

pub use goofi_core::time::Clock;

/// The timing door: what the engine is doing, for `session status`.
pub struct GraphicsStatus {
    pub adapter: String,
    pub backend: String,
    pub clock: &'static str,
    /// How many `Window` nodes have a window open on the machine's screen.
    pub windows: u64,
    pub frames: u64,
    pub stages: u64,
    pub tick_max_us: u64,
}

/// One live node: what the plan reads off it, and the cells its two halves share.
pub(crate) struct Instance {
    pub(crate) class: Arc<Class>,
    pub(crate) params: Arc<[AtomicU64]>,
    pub(crate) uploads: Vec<Arc<Mutex<Option<half::Upload>>>>,
    /// The last upload's size, as the half packs it.
    pub(crate) uploaded: Arc<AtomicU64>,
    pub(crate) readers: Arc<AtomicBool>,
    pub(crate) tap: Arc<Mutex<half::Tap>>,
    /// What this node's readers want its readback fitted into: the bridge writes it, the render
    /// thread reads it, and nothing between the two holds a copy.
    pub(crate) tap_box: Arc<AtomicU64>,
    control: Handle,
    pub(crate) source: Option<producer::Source>,
    program: Option<(Arc<str>, scan::Built)>,
}

impl Instance {
    /// The size this node asks for, from the one writer of its param atomics — a constant and an
    /// evaluated binding alike. 0 on an axis means follow what is wired behind it.
    pub(crate) fn asked(&self) -> (u32, u32) {
        plan::asked(&self.params, self.class.manifest.params.len())
    }
}

pub struct GraphicsEngine {
    iox: Arc<goofi_transport::Iox>,
    instance: String,
    time: Arc<goofi_core::time::Time>,
    clock: Clock,
    shared: Arc<Shared>,
    pub(crate) compiler: Compiler,
    pub(crate) python: Option<goofi_python::catalog::Python>,
    pub(crate) classes: HashMap<String, Arc<Class>>,
    /// The file and stamp each class was registered from, so a rescan skips one that did not
    /// move rather than validating and recompiling every shipped shader for one edit elsewhere.
    pub(crate) stamps: HashMap<String, (std::path::PathBuf, goofi_node::Stamp)>,
    live: HashMap<Uid, Instance>,
    runtime: Arc<Mutex<Runtime>>,
    inbox: Arc<Mutex<Vec<runtime::Cmd>>>,
    stats: Arc<Stats>,
    ticker: Option<(Arc<AtomicBool>, goofi_supervisor::worker::Worker)>,
    faults: goofi_runtime::Faults,
    ui: Option<goofi_window::Ui>,
    /// The window each window node has open, and the size it was last given.
    windows: HashMap<Uid, (goofi_window::Id, (u32, u32))>,
    pending: Vec<(Uid, Status)>,
    dirty: bool,
    /// What the render thread found an armed stage could not record; settled as a fault like any
    /// other, so a missing encoder reaches the panel the way every node failure does.
    troubles: runtime::Troubles,
    /// After the classes and the runtime, for the reason [`Gpu`] states.
    gpu: Arc<Gpu>,
    /// Last: every bell onto a control half is built from it, and fields drop in order.
    bells: goofi_transport::IoxNode,
}

/// One universal `common` param, as a function of the manifest it is added to — the signal
/// engine's shape, because the answer depends on whether the node makes its own frames.
type CommonDecl = fn(&NodeManifest) -> ParamDecl;

fn size_decl(name: &'static str, source: &'static str, m: &NodeManifest) -> ParamDecl {
    ParamDecl {
        group: "common",
        name,
        spec: ParamSpec::Int { default: 0, min: 0, max: plan::MAX_SIZE as i64, options: &[] },
        expression: Some(ExprDecl {
            source,
            mode: if m.producer { ExprMode::On } else { ExprMode::Off },
            trigger: false,
        }),
        doc: Some(
            "Texture size in pixels; 0 follows the first wired texture input, or the frame \
             uploaded to the first array input. A node that makes its own frames follows the \
             patch's default instead.",
        ),
        section: 0,
        show: None,
    }
}

fn width(m: &NodeManifest) -> ParamDecl {
    size_decl("width", "variables.system.default_width", m)
}

fn height(m: &NodeManifest) -> ParamDecl {
    size_decl("height", "variables.system.default_height", m)
}

/// The universal `common` group every graphics node carries; a third param is added here and
/// nowhere else.
static COMMON_DECLS: &[CommonDecl] = &[width, height];

fn common_decls(m: &NodeManifest) -> impl Iterator<Item = ParamDecl> + '_ {
    COMMON_DECLS.iter().map(move |d| d(m))
}

/// Every param a graphics node holds: the author's, then the engine's universal group. ONE order
/// — the param atomics, the desired consts and every binding index are all read against it.
fn decls_of(manifest: &'static NodeManifest) -> Vec<ParamDecl> {
    manifest.params.iter().copied().chain(common_decls(manifest)).collect()
}

impl GraphicsEngine {
    /// Open the device and start the engine, or say why this machine has none.
    pub fn open(
        iox: Arc<goofi_transport::Iox>,
        instance: String,
        time: Arc<goofi_core::time::Time>,
        waker: Arc<DrainWaker>,
        clock: Clock,
    ) -> Result<GraphicsEngine, String> {
        let gpu = gpu::shared()?;
        let shared = Arc::new(Shared::new(waker));
        let stats = Arc::new(Stats::default());
        let troubles = runtime::Troubles::default();
        let runtime = Arc::new(Mutex::new(Runtime::new(
            gpu.clone(),
            time.clone(),
            stats.clone(),
            troubles.clone(),
            shared.clone(),
        )));
        let inbox = runtime.lock().inbox.clone();
        let ticker = (clock == Clock::Internal).then(|| {
            let stop = Arc::new(AtomicBool::new(false));
            let (rt, halt) = (runtime.clone(), stop.clone());
            let thread = goofi_supervisor::worker::thread("goofi-graphics-clock")
                .spawn(move || {
                    let mut next = Instant::now();
                    while !halt.load(Ordering::Relaxed) {
                        // A tick that panics ends no clock: the cause is logged and the next
                        // tick runs; the runtime's lock takes the poison over.
                        if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rt.lock().tick())) {
                            goofi_supervisor::log::record(goofi_supervisor::log::Source::component("graphics"), goofi_supervisor::log::Level::Error, None, format!("the tick panicked: {}", goofi_node::panic_message(p)));
                        }
                        next += PERIOD;
                        // A tick that overran does not try to catch up: the next one is now.
                        match next.checked_duration_since(Instant::now()) {
                            Some(left) => std::thread::sleep(left),
                            None => next = Instant::now(),
                        }
                    }
                })
                .expect("the render clock");
            (stop, thread)
        });
        Ok(GraphicsEngine {
            instance,
            time,
            clock,
            compiler: Compiler::start(gpu.clone(), shared.clone()),
            gpu,
            shared,
            classes: HashMap::new(),
            stamps: HashMap::new(),
            python: None,
            live: HashMap::new(),
            runtime,
            inbox,
            stats,
            ticker,
            faults: goofi_runtime::Faults::default(),
            ui: None,
            windows: HashMap::new(),
            pending: Vec::new(),
            dirty: false,
            troubles,
            bells: iox.node()?,
            iox,
        })
    }

    /// The one recorder an armed stage encodes into. Without it nothing records, which is what a
    /// harness with no recorder is.
    pub fn set_recorder(&self, recorder: Arc<goofi_record::Recorder>) {
        self.ask(runtime::Cmd::Recorder(recorder));
    }

    /// The window thread, where a `Window` node's frames go. Without one there are no windows,
    /// and a `Window` node is a pass-through with a viewer like any other node.
    pub fn set_ui(&mut self, ui: Option<goofi_window::Ui>) {
        self.ask(runtime::Cmd::Ui(ui.clone()));
        self.ui = ui;
    }

    /// Ask the render thread for something. Never blocks: a tick is long, and an op that waited
    /// on one would be an op that waits on a render.
    fn ask(&self, cmd: runtime::Cmd) {
        self.inbox.lock().push(cmd);
    }

    /// The external clock: run `frames` ticks on the caller's thread. The harness's door.
    pub fn render(&mut self, frames: usize) {
        for _ in 0..frames {
            let mut runtime = self.runtime.lock();
            runtime.tick();
            runtime.finish();
        }
    }

    /// The window `uid` has open on the machine's screen, if it is a window node with one.
    pub fn window_of(&self, uid: Uid) -> Option<goofi_window::Id> {
        self.windows.get(&uid).map(|(id, _)| *id)
    }

    pub fn status(&self) -> GraphicsStatus {
        GraphicsStatus {
            adapter: self.gpu.adapter.clone(),
            backend: self.gpu.backend.clone(),
            clock: self.clock.name(),
            windows: self.windows.len() as u64,
            frames: self.stats.frames.load(Ordering::Relaxed),
            stages: self.stats.stages.load(Ordering::Relaxed),
            tick_max_us: self.stats.tick_max_us.load(Ordering::Relaxed),
        }
    }

    /// Everything one node's runtime holds, read off the settled view: a texture wire is a plan
    /// edge, carried by the stage rather than subscribed, and a recording is the render thread's.
    fn desired_of(&self, view: &GraphView<'_>, uid: Uid, nv: &NodeView<'_>) -> Desired {
        let plane = Plane { kind: Some(SlotType::Texture), planned: &|_| false, records: false };
        goofi_runtime::desired_of(view, uid, nv, &decls_of(self.live[&uid].class.manifest), &plane)
    }

    /// The window each window node should have, opened, resized or closed to match settled state.
    /// A window is the frame's own size, so nothing scales and no platform needs a scaler.
    fn follow_windows(&mut self, view: &GraphView<'_>, sizes: &HashMap<Uid, (u32, u32)>) {
        let Some(ui) = self.ui.clone() else { return };
        let want: HashMap<Uid, (u32, u32)> = self
            .live
            .iter()
            .filter(|(_, inst)| inst.class.window)
            .map(|(uid, _)| (*uid, sizes.get(uid).copied().unwrap_or((plan::GENERATOR, plan::GENERATOR))))
            .collect();
        for uid in self.windows.keys().copied().collect::<Vec<_>>() {
            if !want.contains_key(&uid) {
                let (id, _) = self.windows.remove(&uid).expect("just listed");
                ui.post(move |host| host.close_window(id));
            }
        }
        for (uid, size) in want {
            match self.windows.get_mut(&uid) {
                Some(held) if held.1 == size => {}
                Some(held) => {
                    held.1 = size;
                    let id = held.0;
                    ui.post(move |host| host.resize_window(id, size));
                }
                None => {
                    // The node's own name, because a patch may open several.
                    let title = view.nodes.get(&uid).map_or_else(String::new, |nv| nv.name.to_string());
                    let opened =
                        ui.run(move |host| host.open_window(&title, size, Box::new(|_| {})).map(|w| w.id()));
                    match opened {
                        Ok(id) => {
                            self.windows.insert(uid, (id, size));
                        }
                        Err(why) => self.pending.push((
                            uid,
                            Status::Fault {
                                fault: Some(goofi_node::NodeFault::Process {
                                    msg: format!("no window: {why}"),
                                    since: self.time.now(),
                                }),
                            },
                        )),
                    }
                }
            }
        }
    }

    /// How many ARRAY inputs a node has — one upload cell each.
    fn uploads_of(manifest: &NodeManifest) -> usize {
        manifest.inputs.iter().filter(|s| s.kind != SlotType::Texture).count()
    }
}

impl Engine for GraphicsEngine {
    fn rust_sdk(&self) -> Option<&'static str> { Some("goofi-graphics-sdk") }
    fn id(&self) -> &'static str {
        "graphics"
    }

    fn dirty(&self) -> bool {
        self.dirty || self.shared.replan.load(Ordering::Acquire)
    }

    fn library(&self) -> Vec<LibraryEntry> {
        self.classes
            .values()
            .map(|c| LibraryEntry { manifest: c.manifest, isolation: c.isolation })
            .collect()
    }

    fn scan(&mut self, dir: &Path) -> Vec<ScannedType> {
        scan::scan(self, dir)
    }

    fn remove_type(&mut self, type_name: &str) -> bool {
        let gone = self.classes.remove(type_name);
        let held = gone.is_some();
        gpu::give_back(gone);
        held
    }

    fn universal_decls(&self, manifest: &'static NodeManifest) -> Vec<ParamDecl> {
        let mut decls: Vec<_> = common_decls(manifest).collect();
        if self.classes.get(manifest.type_name).is_some_and(|c| matches!(c.kind, scan::Kind::Host(_))) {
            for d in &mut decls { d.expression = None; }
        }
        decls
    }

    fn insert(&mut self, uid: Uid, type_name: &str, generation: u64, params: &ParamGroups) -> Option<String> {
        let Some(class) = self.classes.get(type_name).cloned() else {
            return Some(format!("no graphics node type `{type_name}`"));
        };
        let manifest = class.manifest;
        let atomics: Arc<[AtomicU64]> = decls_of(manifest)
            .iter()
            .map(|d| AtomicU64::new(goofi_runtime::scalar_of(params, d).to_bits()))
            .collect();
        let uploads: Vec<Arc<Mutex<Option<half::Upload>>>> =
            (0..Self::uploads_of(manifest)).map(|_| Arc::new(Mutex::new(None))).collect();
        let uploaded = Arc::new(AtomicU64::new(0));
        let readers = Arc::new(AtomicBool::new(false));
        let tap = Arc::new(Mutex::new(half::Tap::default()));
        let tap_box = Arc::new(AtomicU64::new(0));
        let spawn = goofi_runtime::Spawn {
            engine: "graphics",
            uid,
            instance: self.instance.clone(),
            base: goofi_transport::service_base(&self.instance, uid, generation),
            manifest,
            decls: decls_of(manifest),
            params: atomics.clone(),
            time: self.time.clone(),
        };
        let (cells, flag, out, seen) = (uploads.clone(), readers.clone(), tap.clone(), uploaded.clone());
        let size = manifest.params.len();
        let source = matches!(class.kind, scan::Kind::Host(_)).then(producer::Source::default);
        let factory = match &class.kind {
            scan::Kind::Shader(_) => None,
            scan::Kind::Host(factory) => Some((factory.clone(), source.clone().expect("host source"), params.clone())),
        };
        // The producer is BUILT on the runtime's thread: a Python node's construction executes
        // its module, and this insert holds the graph mutex.
        let make = move || {
            let producer = factory.map(|(f, source, params)| {
                let build: goofi_runtime::NodeBuild = Box::new(move |p| f(p));
                (HostExecutor::new(manifest, decls_of(manifest), build, &params), source)
            });
            GraphicsHalf::new(cells, flag, out, seen, size).with_producer(producer)
        };
        let control = match goofi_runtime::spawn(&self.iox, spawn, self.shared.clone(), &self.bells, make) {
            Ok(handle) => handle,
            Err(e) => return Some(e),
        };
        self.ask(runtime::Cmd::Insert(uid, runtime::params_len(manifest)));
        self.live.insert(uid, Instance { class: class.clone(), params: atomics, uploads, uploaded, readers, tap, tap_box, control, source, program: None });
        self.dirty = true;
        self.shared.waker.notify();
        None
    }

    fn remove(&mut self, uid: Uid) {
        if let Some(inst) = self.live.remove(&uid) {
            self.shared.reports.lock().retain(|(u, _)| *u != uid);
            inst.control.stop();
            self.ask(runtime::Cmd::Remove(uid));
            gpu::give_back(inst);
            self.faults.forget(uid);
            self.troubles.lock().remove(&uid);
            self.pending.retain(|(u, _)| *u != uid);
            self.dirty = true;
        }
    }

    fn settle(&mut self, view: &GraphView<'_>, _touched: &[Touched]) {
        self.dirty = false;
        self.shared.replan.store(false, Ordering::Release);
        for uid in self.live.keys().copied().collect::<Vec<_>>() {
            let Some(nv) = view.nodes.get(&uid) else { continue };
            let desired = self.desired_of(view, uid, nv);
            self.live[&uid].control.send_if_changed(desired);
        }
        // Producers hold CPU descriptions only. Compiled programs belong to the graphics engine.
        for inst in self.live.values_mut() {
            let source = inst.source.as_ref().and_then(|s| {
                s.lock().as_ref().and_then(|frame| match &frame.content {
                    producer::Content::Render(text) => Some(text.clone()),
                    producer::Content::Pixels(_) => None,
                })
            });
            if inst.program.as_ref().map(|(text, _)| text) == source.as_ref() { continue; }
            let next = source.map(|text| {
                let built = self.compiler.program(inst.class.manifest, &text).unwrap_or_else(|why| {
                    let cell = Arc::new(std::sync::OnceLock::new());
                    let _ = cell.set(Err(why));
                    cell
                });
                (text, built)
            });
            gpu::give_back(std::mem::replace(&mut inst.program, next));
        }
        // Windows first: a screen is a reader, so one opened here must be in THIS plan's demand.
        self.follow_windows(view, &plan::sizes(view, &self.live));
        let open: HashMap<Uid, goofi_window::Id> = self.windows.iter().map(|(u, (id, _))| (*u, *id)).collect();
        let (plan, mut faults) = plan::compile(view, &self.live, &open);
        faults.extend(self.troubles.lock().iter().map(|(u, w)| (*u, w.clone())));
        let since = self.time.now();
        self.pending.extend(self.faults.settle(faults, since));
        self.ask(runtime::Cmd::Plan(plan));
        if !self.pending.is_empty() {
            self.shared.waker.notify();
        }
    }

    fn drain(&mut self, apply: &mut dyn FnMut(Uid, Status)) -> usize {
        self.shared.drain(&mut self.pending, apply)
    }

    fn request(&mut self, uid: Uid, request: goofi_node::Request) {
        if let Some(inst) = self.live.get(&uid) {
            match request.kind {
                goofi_node::RequestKind::Refresh => inst.control.refresh(request.key),
                goofi_node::RequestKind::Pulse => inst.control.pulse(request.key),
            }
        }
    }

    /// The tap renders at the size its viewers will reduce it to anyway, so a 4K frame is not
    /// read back only to be averaged down on a CPU. Only the READBACK shrinks: the stage still
    /// renders at its authored `output` size, which is what a window on the screen shows.
    /// It is one cell rather than plan state, because a viewer appearing or leaving must not be
    /// able to re-plan an engine — an accessory never reaches the engine's own scheduling.
    fn view_demand(&mut self, uid: Uid, _slot: &str, want: Option<goofi_view::ViewWant>) {
        if let Some(inst) = self.live.get(&uid) {
            inst.tap_box.store(plan::pack(want), Ordering::Relaxed);
        }
    }

    fn set_evaluator(&mut self, evaluator: Arc<dyn goofi_node::ExprEvaluator>) {
        *self.shared.evaluator.lock() = Some(evaluator);
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    /// Stop the clock and the compiler, then every control half, and WAIT for each to release its
    /// shared memory.
    fn shutdown(&mut self) {
        if let Some((stop, thread)) = self.ticker.take() {
            stop.store(true, Ordering::Relaxed);
            let _ = thread.join();
        }
        self.compiler.stop();
        if let Some(ui) = self.ui.take() {
            for (id, _) in std::mem::take(&mut self.windows).into_values() {
                ui.post(move |host| host.close_window(id));
            }
        }
        let halts: Vec<Arc<goofi_transport::Halt>> = self.live.values().map(|i| i.control.halt.clone()).collect();
        for inst in self.live.values() {
            inst.control.stop();
        }
        goofi_transport::wait_released(halts.iter().map(|h| &**h), goofi_transport::SHUTDOWN_WAIT);
        self.runtime.lock().clear();
        let gate = gpu::gate();
        self.live.clear();
        self.classes.clear();
        drop(gate);
    }
}

impl Drop for GraphicsEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}
