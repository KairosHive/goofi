//! The graphics engine behind the `Engine` seam: shader nodes on the GPU, one device, one render
//! thread, no window. The engine (this file) owns the library, the plan and the clock; the render
//! thread (`runtime`) owns the textures and draws; a node's control half (`half`) is a thread of
//! its own, parked on the node's door — the same one the audio engine uses.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use goofi_supervisor::sync::{Mailbox, Stack};
use std::time::Duration;

use goofi_runtime::{Desired, Handle, Plane, Shared};
use goofi_core::SlotType;
use goofi_node::{Face, DrainWaker, Engine, ExprDecl, ExprMode, GraphView, LibraryEntry, NodeManifest, NodeView,
    ParamDecl, ParamGroups, ParamSpec, ScannedType, Status, Uid,
};

mod gpu;
mod half;
mod montage;
mod plan;
mod runtime;
mod scan;
mod share;
mod shader;
mod resources;

use gpu::Gpu;
use half::GraphicsHalf;
use runtime::{Runtime, Stats};
use scan::Class;
pub use goofi_python::catalog::Python;

/// What `graphics.fps`, the clock's pace, is held to. A tick the clock did not pace — the
/// harness's `render` — is why every frame's instant is written beside the video.
const FPS_RANGE: std::ops::RangeInclusive<u32> = 1..=240;

pub use goofi_supervisor::timing::Clock;

/// The timing door: what the engine is doing, for `session status`.
#[derive(serde::Serialize)]
pub struct GraphicsStatus {
    pub adapter: String,
    pub backend: String,
    /// Whether the adapter times a pass: what a node's share of the frame is read from.
    pub timed: bool,
    /// How many nodes the tick calls hold their pictures in device memory their child maps.
    pub shared: u64,
    pub clock: &'static str,
    /// Whether the clock waits for a reader, a window or a recording: it turns while one draws.
    pub parked: bool,
    /// The rate the clock paces, as `graphics.fps` last settled.
    pub fps: u32,
    /// How many `Window` nodes have a window open on the machine's screen, and how many of
    /// those the GPU presents into directly.
    pub windows: u64,
    pub surfaces: u64,
    /// How many shader types have a pipeline asked of the device: those a node was made of.
    pub pipelines: u64,
    pub frames: u64,
    pub stages: u64,
    pub tick_max_us: u64,
}

/// One live node: what the plan reads off it, and the cells its two halves share.
pub(crate) struct Instance {
    pub(crate) class: Arc<Class>,
    pub(crate) cells: Arc<half::Cells>,
    control: Handle,
    /// The width and height the settled document holds as constants, per axis; `None` where an
    /// expression drives the axis and the half's evaluation is the one value. A fullscreen
    /// window node's is its monitor's size.
    settled_size: (Option<u32>, Option<u32>),
}

impl Instance {
    /// The size this node asks for: a constant as the last settle read it, so the plan decides
    /// from settled state; an evaluated binding from the half's param atomics, its one writer.
    /// 0 on an axis means follow what is wired behind it.
    pub(crate) fn asked(&self) -> (u32, u32) {
        let (w, h) = plan::asked(&self.cells.params, self.class.manifest.params.len());
        (self.settled_size.0.unwrap_or(w), self.settled_size.1.unwrap_or(h))
    }
}

/// A window node's window: the loop's name for it, the size and place it was last given, and the
/// surface the GPU presents into it — none where the screen gives no handles, or the GPU refused one.
pub(crate) struct Opened {
    pub(crate) id: goofi_window::Id,
    size: (u32, u32),
    place: goofi_window::Place,
    pub(crate) surface: Option<Arc<wgpu::Surface<'static>>>,
}

pub struct GraphicsEngine {
    hub: Arc<goofi_transport::Hub>,
    instance: String,
    time: Arc<goofi_supervisor::timing::Time>,
    clock: Clock,
    /// The pace the clock thread keeps, written at settle from `graphics.fps`.
    fps: Arc<AtomicU32>,
    /// The pace a tap is read back at, written at settle from `system.viewer_fps`.
    viewer_fps: Arc<AtomicU32>,
    shared: Arc<Shared>,
    /// The interpreters a Python node is probed with, and the one its child runs in.
    pub(crate) python: Option<Python>,
    pub(crate) classes: HashMap<String, Arc<Class>>,
    /// The file and stamp each class was registered from, so a rescan skips one that did not
    /// move rather than validating and recompiling every shipped shader for one edit elsewhere.
    pub(crate) stamps: HashMap<String, (std::path::PathBuf, goofi_node::Stamp)>,
    /// Every pipeline build still on its thread, a replaced class's too: `shutdown` waits for
    /// them, because a compile inside the driver must not outlive the device or the process.
    pub(crate) compiles: Vec<scan::Built>,
    live: HashMap<Uid, Instance>,
    /// Whose thread ticks the runtime: the clock's own, or the harness's hand. The runtime is
    /// that thread's alone; nothing else holds it.
    ticker: Option<Ticker>,
    inbox: std::sync::mpsc::Sender<runtime::Cmd>,
    /// The clock's door: a command, a reader and a recording ring it, and so does the stop.
    wake: Arc<goofi_supervisor::timing::Event>,
    recording: Arc<AtomicBool>,
    stats: Arc<Stats>,
    faults: goofi_runtime::Faults,
    ui: Option<goofi_window::Ui>,
    windows: HashMap<Uid, Opened>,
    pending: Vec<(Uid, Status)>,
    dirty: bool,
    /// What the render thread found a stage could not do, as it last said it; taken at settle,
    /// kept here, and settled as a fault like any other.
    troubles: runtime::Troubles,
    troubled: HashMap<Uid, String>,
    /// Every node's picture size, as the last settle planned it.
    sizes: HashMap<Uid, (u32, u32)>,
    /// After the classes and the runtime, for the reason [`Gpu`] states.
    gpu: Arc<Gpu>,
}

/// The thread the runtime lives on. Under the internal clock it is the clock thread's, which
/// ticks it at `graphics.fps` and gives every GPU object back before it ends; under the external
/// clock it is here, ticked by [`GraphicsEngine::render`].
enum Ticker {
    Clock { stop: Arc<AtomicBool>, thread: goofi_supervisor::worker::Worker },
    Hand(Box<Runtime>),
}

fn size_decl(name: &'static str, source: &'static str, m: &NodeManifest) -> ParamDecl {
    ParamDecl {
        expression: Some(ExprDecl {
            source,
            mode: if m.inputs.iter().any(|s| s.kind == SlotType::Texture) { ExprMode::Off } else { ExprMode::On },
        }),
        doc: Some(
            "Texture size in pixels; 0 follows the first wired texture input, or the frame \
             uploaded to the first array input. A node with neither follows the patch's \
             default instead.",
        ),
        ..ParamDecl::new("common", name, ParamSpec::Num { default: &[0.0], min: 0.0, max: plan::MAX_SIZE as f64, int: true, options: &[], face: Face::Plain })
    }
}

/// The universal `common` group every shader carries; a third param is added here and nowhere
/// else. A node the tick calls has none: its output is the size of the picture it makes.
fn common_decls(class: &Class) -> Vec<ParamDecl> {
    if class.node() {
        return Vec::new();
    }
    let m = class.manifest;
    vec![size_decl("width", "vars.graphics.default_width", m), size_decl("height", "vars.graphics.default_height", m)]
}

/// Every param a graphics node holds: the author's, then the engine's universal group. ONE order
/// — the param atomics, the desired consts and every binding index are all read against it.
fn decls_of(class: &Class) -> Vec<ParamDecl> {
    class.manifest.params.iter().copied().chain(common_decls(class)).collect()
}

impl GraphicsEngine {
    pub fn set_python(&mut self, python: Python) {
        self.python = Some(python);
    }

    /// What builds the executor of a node the tick calls, on the node's own thread: the child
    /// that runs its source, and its `setup`. None for a shader.
    fn node_build(&self, class: &Class, params: &ParamGroups, cells: &half::Cells) -> Option<impl FnOnce() -> (goofi_runtime::HostExecutor, Vec<usize>) + Send + 'static> {
        let scan::Kind::Node { python, source } = &class.kind else { return None };
        // Where the device shares memory, the child learns the door to join and the device to match.
        let door = cells.node.as_ref().and_then(|meeting| meeting.door.as_ref()).zip(self.gpu.shares);
        let env: Vec<(&'static str, String)> = door
            .map(|(door, uuid)| vec![(share::DOOR, door.name().to_string()), (share::DEVICE, uuid.iter().map(|byte| format!("{byte:02x}")).collect())])
            .unwrap_or_default();
        let (hub, python, source, params) = (self.hub.clone(), python.clone(), source.clone(), params.clone());
        let (manifest, decls) = (class.manifest, decls_of(class));
        // The half's inboxes are the ARRAY inputs, then the TEXTURE ones; a node reads by slot.
        let declared = |name: &str| manifest.inputs.iter().position(|slot| slot.name == name).expect("an input of this manifest");
        let slots: Vec<usize> = shader::array_inputs(manifest).chain(shader::texture_inputs(manifest)).map(declared).collect();
        Some(move || {
            let build: goofi_runtime::NodeBuild = Box::new(move |_| {
                let child = goofi_python::subproc::subproc_in(hub, python, source, env);
                Box::new(goofi_python::subproc::RemoteNode::new(child, goofi_host_sdk::host::in_slots(manifest)))
            });
            (goofi_runtime::HostExecutor::new(manifest, decls, build, &params), slots)
        })
    }

    /// Open the device and start the engine, or say why this machine has none.
    pub fn open(
        hub: Arc<goofi_transport::Hub>,
        instance: String,
        time: Arc<goofi_supervisor::timing::Time>,
        waker: Arc<DrainWaker>,
        clock: Clock,
        recording: Arc<AtomicBool>,
    ) -> Result<GraphicsEngine, String> {
        let gpu = gpu::shared()?;
        let shared = Arc::new(Shared::new(waker));
        let stats = Arc::new(Stats::default());
        let troubles = runtime::Troubles::default();
        let fps = Arc::new(AtomicU32::new(fps_of(None)));
        let viewer_fps = Arc::new(AtomicU32::new(viewer_fps_of(None)));
        let (inbox, asked) = std::sync::mpsc::channel();
        let wake = Arc::new(goofi_supervisor::timing::Event::new());
        let links = runtime::Links { stats: stats.clone(), troubles: troubles.clone(), shared: shared.clone(), recording: recording.clone(), fps: fps.clone(), viewer_fps: viewer_fps.clone() };
        let mut runtime = Runtime::new(gpu.clone(), time.clone(), links, asked);
        let ticker = match clock {
            Clock::External => Ticker::Hand(Box::new(runtime)),
            Clock::Internal => {
                let stop = Arc::new(AtomicBool::new(false));
                let (halt, pace, door, stats) = (stop.clone(), fps.clone(), wake.clone(), stats.clone());
                let thread = goofi_supervisor::worker::spawn("goofi-graphics-clock", move || {
                    let mut grid = goofi_supervisor::timing::Pace::new(Duration::from_secs_f64(1.0 / f64::from(pace.load(Ordering::Relaxed))));
                    while !halt.load(Ordering::Relaxed) {
                        // A tick that panics ends no clock: the cause is logged and the next
                        // tick runs on what the runtime still holds.
                        let drew = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runtime.tick())).unwrap_or_else(|p| {
                            goofi_supervisor::log::record(goofi_supervisor::log::Source::component("graphics"), goofi_supervisor::log::Level::Error, None, format!("the tick panicked: {}", goofi_node::panic_message(p)));
                            true
                        });
                        // Nothing asked for a stage: the clock parks until its door rings.
                        if !drew && !halt.load(Ordering::Relaxed) {
                            stats.parked.store(true, Ordering::Relaxed);
                            door.wait();
                            stats.parked.store(false, Ordering::Relaxed);
                            continue;
                        }
                        grid.set_period(Duration::from_secs_f64(1.0 / f64::from(pace.load(Ordering::Relaxed))));
                        // A tick that overran does not try to catch up: the grid restarts from now.
                        goofi_supervisor::timing::sleep_until(grid.advance());
                    }
                    // Every GPU object goes back on the thread that made it, before the
                    // engine closes the windows the surfaces were made on.
                    runtime.clear();
                })
                .expect("the render clock");
                Ticker::Clock { stop, thread }
            }
        };
        Ok(GraphicsEngine {
            instance,
            time,
            clock,
            fps,
            viewer_fps,
            gpu,
            shared,
            python: None,
            classes: HashMap::new(),
            stamps: HashMap::new(),
            compiles: Vec::new(),
            live: HashMap::new(),
            inbox,
            wake,
            recording,
            stats,
            ticker: Some(ticker),
            faults: goofi_runtime::Faults::default(),
            ui: None,
            windows: HashMap::new(),
            pending: Vec::new(),
            dirty: false,
            troubles,
            troubled: HashMap::new(),
            sizes: HashMap::new(),
            hub,
        })
    }

    /// Ask every node's half for one tick, acknowledged: what a recording's stop waits for, so a
    /// readback the half still holds reaches the queue before the file closes.
    pub fn flush_recording(&self) -> Vec<std::sync::mpsc::Receiver<Result<(), String>>> {
        self.live.values().map(|inst| inst.control.flush()).collect()
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
        let _ = self.inbox.send(cmd);
        self.wake.ring(0);
    }

    /// The external clock: run `frames` ticks on the caller's thread. The harness's door; a no-op
    /// under the internal clock, whose thread holds the runtime.
    pub fn render(&mut self, frames: usize) {
        let Some(Ticker::Hand(runtime)) = &mut self.ticker else { return };
        for _ in 0..frames {
            runtime.tick();
            runtime.finish();
        }
    }

    /// The picture `uid` makes, as the last settle sized it, and the rate the clock paces: what a
    /// recording of it is opened at.
    pub fn picture(&self, uid: Uid) -> Option<((u32, u32), f64)> {
        self.sizes.get(&uid).map(|size| (*size, f64::from(self.fps.load(Ordering::Relaxed))))
    }

    /// The window `uid` has open on the machine's screen, if it is a window node with one.
    pub fn window_of(&self, uid: Uid) -> Option<goofi_window::Id> {
        self.windows.get(&uid).map(|o| o.id)
    }

    pub fn status(&self) -> GraphicsStatus {
        GraphicsStatus {
            adapter: self.gpu.adapter_name.clone(),
            backend: self.gpu.backend.clone(),
            timed: self.gpu.device.features().contains(wgpu::Features::TIMESTAMP_QUERY),
            shared: self.live.values().filter(|inst| inst.cells.node.as_ref().is_some_and(|meeting| meeting.linked.load(Ordering::Relaxed))).count() as u64,
            clock: self.clock.name(),
            parked: self.stats.parked.load(Ordering::Relaxed),
            fps: self.fps.load(Ordering::Relaxed),
            windows: self.windows.len() as u64,
            surfaces: self.windows.values().filter(|o| o.surface.is_some()).count() as u64,
            pipelines: self.classes.values().filter(|c| matches!(&c.kind, scan::Kind::Shader(s) if s.built().is_some())).count() as u64,
            frames: self.stats.frames.load(Ordering::Relaxed),
            stages: self.stats.stages.load(Ordering::Relaxed),
            tick_max_us: self.stats.tick_max_us.load(Ordering::Relaxed),
        }
    }

    /// Everything one node's runtime holds, read off the settled view: a texture wire is a plan
    /// edge, carried by the stage rather than subscribed; an armed output's queue is the half's,
    /// fed with what the render thread read back.
    fn desired_of(&self, view: &GraphView<'_>, uid: Uid, nv: &NodeView<'_>) -> Desired {
        let plane = Plane { kind: self.carried(), planned: &|_| false, records: true };
        let manifest = self.live[&uid].class.manifest;
        let mut desired = goofi_runtime::desired_of(view, uid, nv, &decls_of(&self.live[&uid].class), &plane);
        // A texture input another engine feeds is a frame off the wire, as an ARRAY input's is.
        let arrays = shader::array_inputs(manifest).count();
        for (t, slot) in shader::texture_inputs(manifest).enumerate() {
            for (wire, (producer, out)) in view.wires_into(uid, slot).filter(|(p, _)| !self.live.contains_key(p)).enumerate() {
                if let Some(service) = goofi_transport::output_of(view, producer, out) {
                    desired.subs.push(goofi_runtime::Sub::Slot { inbox: arrays + t, wire, service, source: String::new() });
                }
            }
        }
        desired
    }

    /// The size of the monitor a fullscreen window node covers; none for any other node, and none
    /// where the machine has no such screen, which the window's opening then reports.
    fn covered(&self, inst: &Instance, nv: &NodeView<'_>) -> Option<(u32, u32)> {
        let place = place_of(nv);
        if !inst.class.window || !place.fullscreen {
            return None;
        }
        self.ui.as_ref()?.run(|host| host.monitors()).get(place.screen).map(|m| m.size)
    }

    /// The window each window node should have, opened, resized or closed to match settled state,
    /// and the nodes whose window would not open. A window is the frame's own size, so nothing
    /// scales and no platform needs a scaler; a new place opens it anew.
    fn follow_windows(&mut self, view: &GraphView<'_>, sizes: &HashMap<Uid, (u32, u32)>) -> Vec<(Uid, String)> {
        let mut refused = Vec::new();
        let Some(ui) = self.ui.clone() else { return refused };
        let want: HashMap<Uid, ((u32, u32), goofi_window::Place)> = self
            .live
            .iter()
            .filter(|(_, inst)| inst.class.window)
            .map(|(uid, _)| {
                let size = sizes.get(uid).copied().unwrap_or_else(|| plan::generator(view));
                (*uid, (size, view.nodes.get(uid).map(place_of).unwrap_or_default()))
            })
            .collect();
        for uid in self.windows.keys().copied().collect::<Vec<_>>() {
            if !want.contains_key(&uid) {
                let opened = self.windows.remove(&uid).expect("just listed");
                drop(opened.surface);
                let id = opened.id;
                ui.post(move |host| host.close_window(id));
            }
        }
        for (uid, (size, place)) in want {
            if self.windows.get(&uid).is_some_and(|held| held.place != place) {
                let opened = self.windows.remove(&uid).expect("just found");
                drop(opened.surface);
                let id = opened.id;
                ui.post(move |host| host.close_window(id));
            }
            match self.windows.get_mut(&uid) {
                Some(held) if held.size == size => {}
                Some(held) => {
                    held.size = size;
                    let id = held.id;
                    ui.post(move |host| host.resize_window(id, size));
                }
                None => {
                    // The node's own name, because a patch may open several.
                    let title = view.nodes.get(&uid).map_or_else(String::new, |nv| nv.name.to_string());
                    let gpu = self.gpu.clone();
                    // The surface is made where the handles are valid: on the window thread.
                    let opened = ui.run(move |host| {
                        let id = host.open_window(&title, size, place, Box::new(|_| {}))?.id();
                        let surface = host.handles(id).map(|h| gpu.surface(h)).transpose().unwrap_or_else(|why| {
                            goofi_supervisor::log::record(goofi_supervisor::log::Source::component("graphics"), goofi_supervisor::log::Level::Warning, None, format!("graphics: `{title}` is presented through the CPU: {why}"));
                            None
                        });
                        Ok::<_, String>((id, surface.map(Arc::new)))
                    });
                    match opened {
                        Ok((id, surface)) => {
                            self.windows.insert(uid, Opened { id, size, place, surface });
                        }
                        Err(why) => refused.push((uid, format!("no window: {why}"))),
                    }
                }
            }
        }
        refused
    }
}

impl Engine for GraphicsEngine {
    fn id(&self) -> &'static str {
        "graphics"
    }

    fn dirty(&self) -> bool {
        self.dirty || self.shared.replan.load(Ordering::Acquire)
    }

    fn carried(&self) -> Option<SlotType> {
        Some(SlotType::Texture)
    }

    fn library(&self) -> Vec<LibraryEntry> {
        self.classes
            .values()
            .map(|c| LibraryEntry { manifest: c.manifest, isolation: c.isolation })
            .collect()
    }

    /// The probes a scan of `dir` would spawn, as work for off the lock.
    fn prepare(&self, dir: &Path) -> Option<Box<dyn FnOnce() + Send>> {
        let python = self.python.clone()?;
        let files: Vec<(std::path::PathBuf, String)> = goofi_node::node_files(dir, "graphics")
            .into_iter()
            .filter(|(path, _, _)| path.extension().is_some_and(|ext| ext == "py"))
            .map(|(path, name, _)| (path, name))
            .collect();
        (!files.is_empty()).then(|| Box::new(move || goofi_python::catalog::warm(&files, &python)) as Box<dyn FnOnce() + Send>)
    }

    fn scan(&mut self, dir: &Path) -> Vec<ScannedType> {
        scan::scan(self, dir)
    }

    fn remove_type(&mut self, type_name: &str) -> bool {
        self.classes.remove(type_name).is_some()
    }

    fn universal_decls(&self, manifest: &'static NodeManifest) -> Vec<ParamDecl> {
        self.classes.get(manifest.type_name).map(|class| common_decls(class)).unwrap_or_default()
    }

    fn insert(&mut self, uid: Uid, type_name: &str, generation: u64, params: &ParamGroups) -> Option<String> {
        let Some(class) = self.classes.get(type_name).cloned() else {
            return Some(format!("no graphics node type `{type_name}`"));
        };
        // The first node of a type asks the device for its pipeline; a type nobody uses costs none.
        if let scan::Kind::Shader(shader) = &class.kind {
            if let Some(build) = shader.start(&self.gpu, &self.shared) {
                self.compiles.retain(|build| !build.is_ready());
                self.compiles.push(build);
            }
        }
        let manifest = class.manifest;
        let base = goofi_transport::service_base(&self.instance, uid, generation);
        let decls = decls_of(&class);
        let cells = Arc::new(half::Cells {
            params: goofi_runtime::cells_of(params, &decls).into(),
            uploads: (0..manifest.inputs.len()).map(|_| Mailbox::new()).collect(),
            spent: Stack::new(),
            uploaded: (0..manifest.inputs.len()).map(|_| AtomicU64::new(0)).collect(),
            readers: AtomicBool::new(false),
            tap: Mailbox::new(),
            tap_box: AtomicU64::new(0),
            record: Stack::new(),
            frame_share: AtomicU64::new(f64::NAN.to_bits()),
            node: class.node().then(|| half::Meeting::new(self.gpu.shares.and_then(|_| share::Door::open()))),
            produced: AtomicU64::new(0),
            // The half's own door: the render thread rings it once a readback is in a cell.
            bell: goofi_transport::Doorbell::on(self.hub.door(&goofi_transport::door_service(&base))),
            wake: self.wake.clone(),
            recording: self.recording.clone(),
        });
        let spawn = goofi_runtime::Spawn {
            engine: "graphics",
            uid,
            base,
            manifest,
            decls,
            params: cells.params.clone(),
            time: self.time.clone(),
        };
        let shared = cells.clone();
        let size = manifest.params.len();
        let node = self.node_build(&class, params, &cells);
        let make = move || GraphicsHalf::new(shared, size, node.map(|build| build()));
        let control = match goofi_runtime::spawn(&self.hub, spawn, self.shared.clone(), make) {
            Ok(handle) => handle,
            Err(e) => return Some(e),
        };
        // A node's params are its child's; only a shader has a uniform block of them.
        self.ask(runtime::Cmd::Insert(uid, if class.node() { 0 } else { runtime::params_len(manifest) }));
        self.live.insert(uid, Instance { class: class.clone(), cells, control, settled_size: (None, None) });
        self.dirty = true;
        self.shared.waker.notify();
        None
    }

    fn remove(&mut self, uid: Uid) {
        if let Some(inst) = self.live.remove(&uid) {
            // Before the half is stopped: a tick that waits on its node must not wait on.
            if let Some(meeting) = &inst.cells.node {
                meeting.close();
            }
            inst.control.stop();
            self.ask(runtime::Cmd::Remove(uid));
            drop(inst);
            self.faults.forget(uid);
            self.troubled.remove(&uid);
            self.pending.retain(|(u, _)| *u != uid);
            self.dirty = true;
        }
    }

    fn settle(&mut self, view: &GraphView<'_>, changed: &[Uid]) {
        self.dirty = false;
        self.shared.replan.store(false, Ordering::Release);
        self.fps.store(fps_of(view.variables.get("graphics.fps")), Ordering::Relaxed);
        self.viewer_fps.store(viewer_fps_of(view.variables.get("system.viewer_fps")), Ordering::Relaxed);
        for uid in changed.iter().copied().filter(|u| self.live.contains_key(u)).collect::<Vec<_>>() {
            let Some(nv) = view.nodes.get(&uid) else { continue };
            let desired = self.desired_of(view, uid, nv);
            // The universal group ends the decls: width, then height. A constant is read here,
            // settled; a bound axis is left to the half, which writes what it evaluates.
            let n = desired.consts.len();
            let settled = |i: usize| {
                let bound = desired.subs.iter().any(|s| matches!(s, goofi_runtime::Sub::Bind { param, .. } if *param == i));
                (!bound).then(|| goofi_runtime::scalar(&desired.consts[i]).clamp(0.0, plan::MAX_SIZE as f64) as u32)
            };
            let covered = self.covered(&self.live[&uid], nv);
            let inst = self.live.get_mut(&uid).expect("listed above");
            inst.settled_size = match covered {
                Some((w, h)) => (Some(w), Some(h)),
                // A node the tick calls has no size params: its picture is the size it makes.
                None if inst.class.node() => (None, None),
                None => (settled(n - 2), settled(n - 1)),
            };
            inst.control.send_if_changed(desired);
        }
        // Windows first: a screen is a reader, so one opened here must be in THIS plan's demand.
        let sizes = plan::sizes(view, &self.live);
        let refused = self.follow_windows(view, &sizes);
        self.sizes = sizes;
        let (plan, mut faults) = plan::compile(view, &self.live, &self.windows, &self.gpu);
        faults.extend(refused);
        // The render thread's word, as it last said it; a node gone since is not in the plan.
        if let Some(said) = self.troubles.take() {
            self.troubled = *said;
        }
        faults.extend(self.troubled.iter().filter(|(u, _)| self.live.contains_key(u)).map(|(u, w)| (*u, w.clone())));
        let since = self.time.now();
        self.pending.extend(self.faults.settle(faults, since));
        self.ask(runtime::Cmd::Plan(plan));
        if !self.pending.is_empty() {
            self.shared.waker.notify();
        }
    }

    fn drain(&mut self, apply: &mut dyn FnMut(Uid, Status)) -> usize {
        self.shared.drain(&mut self.pending, &|u, born| self.live.get(&u).is_some_and(|i| i.control.born() == born), apply)
    }

    fn request(&mut self, uid: Uid, request: goofi_node::Request) {
        if let Some(inst) = self.live.get(&uid) {
            inst.control.request(request);
        }
    }

    /// The tap renders at the size its viewers will reduce it to anyway, so a 4K frame is not
    /// read back only to be averaged down on a CPU. Only the READBACK shrinks: the stage still
    /// renders at its authored `output` size, which is what a window on the screen shows.
    /// It is one cell rather than plan state, because a viewer appearing or leaving must not be
    /// able to re-plan an engine — an accessory never reaches the engine's own scheduling.
    fn view_demand(&mut self, uid: Uid, _slot: &str, want: Option<goofi_view::ViewWant>) {
        if let Some(inst) = self.live.get(&uid) {
            inst.cells.tap_box.store(plan::pack(want), Ordering::Relaxed);
        }
    }

    fn set_evaluator(&mut self, evaluator: Arc<dyn goofi_node::ExprEvaluator>) {
        *self.shared.evaluator.lock() = Some(evaluator);
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    /// Stop the clock, then every control half, and WAIT for each to release its shared memory
    /// and for every pipeline build to leave the driver.
    fn shutdown(&mut self) {
        // A tick that waits on a node goes on, or the clock's thread would never be joined.
        for meeting in self.live.values().filter_map(|inst| inst.cells.node.as_ref()) {
            meeting.close();
        }
        // The surfaces go before their windows: the runtime's chains first, then the engine's.
        match self.ticker.take() {
            Some(Ticker::Clock { stop, thread }) => {
                stop.store(true, Ordering::Relaxed);
                self.wake.ring(0);
                let _ = thread.join();
            }
            Some(Ticker::Hand(mut runtime)) => runtime.clear(),
            None => {}
        }
        goofi_runtime::stop_all(self.live.values().map(|i| &i.control));
        let windows = std::mem::take(&mut self.windows);
        if let Some(ui) = self.ui.take() {
            for opened in windows.into_values() {
                drop(opened.surface);
                let id = opened.id;
                ui.post(move |host| host.close_window(id));
            }
        }
        self.live.clear();
        self.classes.clear();
        for build in self.compiles.drain(..) {
            build.wait();
        }
    }
}

/// `graphics.fps` as the clock reads it: the variable held to [`FPS_RANGE`], or its default.
fn fps_of(held: Option<&goofi_core::Data>) -> u32 {
    let asked = held.map_or(goofi_core::variables::DEFAULT_FPS, |d| goofi_core::control::number_of(d) as i64);
    asked.clamp(i64::from(*FPS_RANGE.start()), i64::from(*FPS_RANGE.end())) as u32
}

/// `system.viewer_fps` as the tap grid reads it: the cap the reducer serves at, held to
/// [`FPS_RANGE`] as the reducer holds it to at least one, or its default.
fn viewer_fps_of(held: Option<&goofi_core::Data>) -> u32 {
    let asked = held.map_or(goofi_core::variables::DEFAULT_VIEWER_FPS, goofi_core::control::number_of);
    let asked = if asked.is_finite() { asked.round() as i64 } else { 1 };
    asked.clamp(i64::from(*FPS_RANGE.start()), i64::from(*FPS_RANGE.end())) as u32
}

impl Drop for GraphicsEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Where a window node's window opens, from its `window` params.
fn place_of(nv: &NodeView<'_>) -> goofi_window::Place {
    let param = |name: &str| nv.params.get("window").and_then(|g| g.get(name));
    goofi_window::Place {
        screen: param("screen").and_then(goofi_core::Param::as_f64).map_or(0, |s| s.max(0.0) as usize),
        fullscreen: param("fullscreen").and_then(goofi_core::Param::as_bool).unwrap_or(false),
    }
}
