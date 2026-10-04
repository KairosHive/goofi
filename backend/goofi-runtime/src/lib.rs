//! One runtime per node of a scheduled engine: a thread parked on the node's door that holds its
//! desired state, bindings, ports and reports. What a run does is the engine's, behind [`Executor`].

use std::collections::{BTreeMap, HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use goofi_core::{Data, Param, SlotType};
use goofi_node::{
    BindingId, BindingView, DrainWaker, EventId, ExprEvaluator, Expression, GraphView, NodeFault, NodeManifest,
    NodeStage, NodeView, ParamDecl, ParamGroups, ParamKey, Status, Uid, Var,
};
use goofi_supervisor::sync::Mutex;
use goofi_transport::{
    door_service, event_service, open_output_subscriber, output_service, publisher, record_door_service,
    record_service, record_shape, stream_service, take_where, BytePublisher, ByteService, ByteSubscriber, Doorbell,
    Halt, Iox, IoxNode, Listener, ServiceKind, ServiceName, INITIAL_SLICE,
};
use indexmap::IndexMap;

mod common;
pub mod host;
pub mod hosted;

pub use common::*;
pub use host::{HostExecutor, NodeBuild};

/// The pace of the paced duties: an executor's tick, a binding with no stream re-evaluated, a
/// pulse lowered.
pub const TICK: Duration = Duration::from_millis(10);

/// The scheduling namespace: a `common.*` param decides WHEN a node runs, so an arrival on it is
/// never a reason to run.
pub const COMMON: &str = "common";

/// How often a node reports its measured update rate. A rate is a MEASUREMENT rather than a
/// transition, and an uncapped producer would take one per emit.
const UFREQ_REPORT: Duration = Duration::from_millis(250);

/// What the engine wants a node's runtime to hold — the WHOLE of it, sent when it changes.
#[derive(Clone, PartialEq)]
pub struct Desired {
    /// The record value per param: what an unbound param reads, and the type a binding coerces to.
    pub consts: Vec<Param>,
    pub subs: Vec<Sub>,
    /// Per output: the doors it rings, by name, once something is published on it.
    pub targets: Vec<Vec<(String, EventId)>>,
    /// The output slots armed for recording, by name, each with its arming's serial.
    pub record: Vec<(String, u64)>,
}

#[derive(Clone, PartialEq)]
pub enum Sub {
    /// One wire into an Array input: the producer service, the inbox its frames enter, its
    /// position among the inbox's wires, and the `node.slot` it comes from.
    Slot { inbox: usize, wire: usize, service: String, source: String },
    /// A binding this runtime evaluates: everything the engine's own plan does not carry. `elem`
    /// names ONE dimension of a vector param, which is all that binding writes.
    Bind { param: usize, elem: Option<usize>, key: ParamKey, source: String, id: Option<BindingId>, vars: Vec<(String, Var)>, trigger: bool },
}

/// What an engine's own plan carries, so the runtime subscribes to the rest: the input kind whose
/// wires are plan edges, the bindings that are, and whether the runtime records the outputs.
pub struct Plane<'a> {
    pub kind: Option<SlotType>,
    pub planned: &'a dyn Fn(&BindingView<'_>) -> bool,
    pub records: bool,
}

/// Everything one node's runtime holds, read off the settled view: the constants per `decls`,
/// the wires and bindings it subscribes, the doors each output rings, and the armed slots.
pub fn desired_of(view: &GraphView<'_>, uid: Uid, nv: &NodeView<'_>, decls: &[ParamDecl], plane: &Plane<'_>) -> Desired {
    let carried = |s: &goofi_node::SlotDecl| plane.kind == Some(s.kind);
    let consts = decls.iter().map(|d| param_of(nv.params, d)).collect();
    // A feed param names a variable: that variable's wire enters the slot the role names.
    let feeds: Vec<(&str, &str)> = goofi_node::feed_decls(nv.manifest)
        .filter_map(|(slot, d)| match goofi_node::param(nv.params, d.group, d.name) {
            Some(goofi_core::Param::Str { value, .. }) if view.variables.contains(value) => Some((slot, value.as_str())),
            _ => None,
        })
        .collect();
    let mut subs = Vec::new();
    let mut inbox = 0;
    for s in nv.manifest.inputs {
        if carried(s) {
            continue;
        }
        let mut wire = 0;
        for (producer, out) in view.wires_into(uid, s.name) {
            if let Some(service) = goofi_transport::output_of(view, producer, out) {
                let source = match (s.multi, view.nodes.get(&producer)) {
                    (true, Some(p)) => format!("{}.{out}", p.name),
                    _ => String::new(),
                };
                subs.push(Sub::Slot { inbox, wire, service, source });
            }
            wire += 1;
        }
        for (_, name) in feeds.iter().filter(|(slot, _)| *slot == s.name) {
            let service = goofi_transport::output_service(&goofi_transport::variables_base(view.instance), name);
            subs.push(Sub::Slot { inbox, wire, service, source: format!("variables.{name}") });
            wire += 1;
        }
        inbox += 1;
    }
    for (param, d) in decls.iter().enumerate() {
        // A param may carry a binding of its own AND one per element; each is subscribed alone.
        for b in nv.bindings.iter().filter(|b| b.live && !(plane.planned)(b) && b.key.group == d.group) {
            let (base, elem) = goofi_node::element(&b.key.name);
            if base != d.name {
                continue;
            }
            let vars = b.vars.iter().map(|v| goofi_transport::var_of(view, v)).collect();
            subs.push(Sub::Bind { param, elem, key: b.key.clone(), source: b.rewritten.to_string(), id: b.id, vars, trigger: b.trigger });
        }
    }
    // A ring would wake a same-engine consumer for what its plan already carries; a consumer on
    // another engine subscribes to the wire whatever its slot's kind.
    let rides = |r: &goofi_node::Ringer<'_>| match r.via {
        goofi_node::Via::Slot(s) => view
            .nodes
            .get(&r.consumer)
            .filter(|c| c.engine == nv.engine)
            .is_some_and(|c| c.manifest.inputs.iter().any(|i| i.name == s && carried(i))),
        goofi_node::Via::Binding(b) => (plane.planned)(b),
    };
    let targets = nv
        .manifest
        .outputs
        .iter()
        .map(|o| goofi_transport::targets_of(view, uid, o.name, view.ringers(uid, o.name).into_iter().filter(|r| !rides(r))))
        .collect();
    let record = if plane.records { nv.recorded.iter().map(|o| (o.slot.clone(), o.serial)).collect() } else { Vec::new() };
    Desired { consts, subs, targets, record }
}

/// What every runtime of one engine shares. An engine's own additions live beside it, in a
/// struct of its own that only its [`Executor`] sees.
pub struct Shared {
    pub evaluator: Mutex<Option<Arc<dyn ExprEvaluator>>>,
    pub reports: Mutex<Vec<(Uid, Status)>>,
    pub waker: Arc<DrainWaker>,
    /// A runtime saw something only a settle can act on — a shape moved, a file opened.
    pub replan: AtomicBool,
}

impl Shared {
    pub fn new(waker: Arc<DrainWaker>) -> Shared {
        Shared { evaluator: Mutex::new(None), reports: Mutex::new(Vec::new()), waker, replan: AtomicBool::new(false) }
    }

    fn report(&self, uid: Uid, status: Status) {
        self.reports.lock().push((uid, status));
        self.waker.notify();
    }

    /// Ask the engine for a settle, and wake the drain that runs one.
    pub fn ask_settle(&self) {
        self.replan.store(true, Ordering::Release);
        self.waker.notify();
    }

    /// Hand the engine's own pending statuses and every runtime's reports to `apply`, and answer
    /// how many. ONE lock over the reports, so nothing lands between a read and a clear.
    pub fn drain(&self, pending: &mut Vec<(Uid, Status)>, apply: &mut dyn FnMut(Uid, Status)) -> usize {
        let mut all = std::mem::take(pending);
        all.append(&mut self.reports.lock());
        let n = all.len();
        for (uid, status) in all {
            apply(uid, status);
        }
        n
    }
}

/// What a scheduled engine's nodes are faulted with, and the deltas a settle owes the graph. The
/// engine states the WHOLE current set each settle; this answers only what moved.
#[derive(Default)]
pub struct Faults(IndexMap<Uid, String>);

impl Faults {
    pub fn settle(&mut self, now: impl IntoIterator<Item = (Uid, String)>, since: f64) -> Vec<(Uid, Status)> {
        let now: IndexMap<Uid, String> = now.into_iter().collect();
        let mut out: Vec<(Uid, Status)> =
            self.0.keys().filter(|u| !now.contains_key(*u)).map(|u| (*u, Status::Fault { fault: None })).collect();
        for (uid, msg) in &now {
            if self.0.get(uid) != Some(msg) {
                let fault = Some(NodeFault::Process { msg: msg.clone(), since });
                out.push((*uid, Status::Fault { fault }));
            }
        }
        self.0 = now;
        out
    }

    pub fn forget(&mut self, uid: Uid) {
        self.0.shift_remove(&uid);
    }
}

/// A scheduled engine's order: Kahn's, feedback nodes first and then by uid, skipping `dropped`,
/// and the nodes in a loop with no feedback node, which the order then leaves out.
pub fn schedule(inbound: &HashMap<Uid, Vec<Uid>>, dropped: &HashSet<Uid>, feedback: impl Fn(Uid) -> bool) -> (Vec<Uid>, HashSet<Uid>) {
    let kahn = |dropped: &HashSet<Uid>| {
        let mut indegree: HashMap<Uid, usize> = HashMap::new();
        let mut successors: HashMap<Uid, Vec<Uid>> = HashMap::new();
        for (uid, from) in inbound.iter().filter(|(u, _)| !dropped.contains(u)) {
            let from: Vec<Uid> = from.iter().copied().filter(|p| !dropped.contains(p)).collect();
            indegree.insert(*uid, from.len());
            for p in from {
                successors.entry(p).or_default().push(*uid);
            }
        }
        let mut order: Vec<Uid> = Vec::with_capacity(indegree.len());
        let mut ready: Vec<Uid> = indegree.iter().filter(|(_, d)| **d == 0).map(|(u, _)| *u).collect();
        while !ready.is_empty() {
            ready.sort_by_key(|u| std::cmp::Reverse((!feedback(*u), u.0)));
            let u = ready.pop().expect("not empty");
            order.push(u);
            for s in successors.get(&u).into_iter().flatten() {
                let d = indegree.get_mut(s).expect("a successor is in the graph");
                *d -= 1;
                if *d == 0 {
                    ready.push(*s);
                }
            }
        }
        let stuck: Vec<Uid> = indegree.keys().filter(|u| !order.contains(u)).copied().collect();
        (order, stuck)
    };
    let (order, stuck) = kahn(dropped);
    // A node Kahn could not place is IN a loop when it reaches itself; the rest are only fed by one.
    let members: HashSet<Uid> = stuck.iter().copied().filter(|u| reaches_itself(*u, inbound, &stuck)).collect();
    let order = if members.is_empty() { order } else { kahn(&members.union(dropped).copied().collect()).0 };
    (order, members)
}

fn reaches_itself(start: Uid, inbound: &HashMap<Uid, Vec<Uid>>, within: &[Uid]) -> bool {
    let mut seen: HashSet<Uid> = HashSet::new();
    let mut stack: Vec<Uid> = inbound.get(&start).into_iter().flatten().copied().filter(|p| within.contains(p)).collect();
    while let Some(u) = stack.pop() {
        if u == start {
            return true;
        }
        if seen.insert(u) {
            stack.extend(inbound.get(&u).into_iter().flatten().copied().filter(|p| within.contains(p)));
        }
    }
    false
}

/// This run's settled state, as [`Executor::run`] reads it.
pub struct Cx<'a> {
    /// The live typed value per declared param — a constant, or what its binding last evaluated to.
    pub values: &'a [Param],
    /// The record value per declared param.
    pub consts: &'a [Param],
    /// The live scalar per declared param, as a DSP thread reads it.
    pub params: &'a [AtomicU64],
    /// Per output: whether anyone subscribes to its data service right now.
    pub readers: &'a [bool],
    /// Per output: whether the recorder holds this slot armed.
    pub recorded: &'a [bool],
    /// The patch's time at this run.
    pub now: f64,
    /// The patch's clock, read again as a frame leaves, for its `emit` stamp.
    pub time: &'a goofi_core::time::Time,
    /// The node this run is, as its frames' `source` names it.
    pub uid: Uid,
}

/// What an executor puts on one output.
pub enum Out<'a> {
    /// Encoded, for the wire alone.
    Bytes(&'a [u8]),
    /// A frame for the wire and, while the slot is armed, its recording.
    Frame(&'a Data),
    /// Encoded, for the recording alone. A frame the segment refuses is a gap in `Meta::index`,
    /// which is what the recorder counts drops by.
    Record(&'a [u8]),
}

/// What is wrong with the executor's node: its `setup` failed and nothing runs until a retry
/// succeeds, or a run did.
#[derive(Clone, Debug, PartialEq)]
pub enum Fault {
    Setup(String),
    Process(String),
}

/// What one call into an [`Executor`] changed. The errors are the WHOLE current set for the keys
/// the executor owns; the runtime files only what moved.
#[derive(Default)]
pub struct Ticked {
    pub errors: Vec<(ParamKey, Option<String>)>,
    /// Only a settle can finish what this call found.
    pub replan: bool,
    /// The node's measured update rate, when a run emitted.
    pub ufreq: Option<f64>,
}

/// The engine's half of a node's runtime: what an arrival becomes, and what goes out.
pub trait Executor {
    /// A frame arrived on wire `wire` of Array input `inbox`; `true` asks for a settle.
    fn arrive(&mut self, inbox: usize, wire: usize, frame: &Data) -> bool;
    /// Whether only the NEWEST frame on an input matters: yes is handed one per pass, and no (an
    /// executor that accumulates every sample, as audio's resampling inbox does) is handed all.
    fn latest_only(&self) -> bool {
        false
    }
    /// The wires into Array input `inbox` moved: the full set, as `(service, node.slot)`.
    fn rewire(&mut self, _inbox: usize, _wires: &[(ServiceName, String)]) {}
    /// The live values moved, or an arrival on a triggering binding asks for a run.
    fn params_changed(&mut self, _values: &[Param], _trigger: bool) -> Ticked {
        Ticked::default()
    }
    /// A pulse param was raised: by the op, or by its source's rising edge.
    fn pulse(&mut self, _param: usize) -> Ticked {
        Ticked::default()
    }
    /// The options behind a refreshable `Str` param, re-enumerated.
    fn refresh(&mut self, _param: usize) -> Option<Vec<String>> {
        None
    }
    /// When the next run is due, given when the last one was; `None` parks until something rings.
    fn next_wake(&self, last_run: Instant) -> Option<Instant> {
        Some(last_run + TICK)
    }
    /// The run: publish what each output holds, and say what changed.
    fn run(&mut self, cx: &Cx<'_>, publish: &mut dyn FnMut(usize, Out<'_>)) -> Ticked;
    /// The executor's standing fault, read after every call.
    fn fault(&self) -> Option<Fault> {
        None
    }
    fn stage(&self) -> NodeStage {
        NodeStage::Ready
    }
}

/// What the engine leaves for a runtime: its whole desired state, the refreshes asked, and the
/// pulses fired.
#[derive(Default)]
struct Mail {
    desired: Option<Desired>,
    refresh: Vec<ParamKey>,
    pulse: Vec<ParamKey>,
    flush: Vec<Flush>,
    /// The thread is gone, so nothing posted here is read again.
    closed: bool,
}

struct Flush {
    armed: Vec<String>,
    ack: std::sync::mpsc::SyncSender<Result<(), String>>,
}

/// The engine's end of one runtime.
pub struct Handle {
    mail: Arc<Mutex<Mail>>,
    halt: Arc<Halt>,
    bell: Doorbell,
    /// What was last sent, so a settle that changes nothing says nothing.
    last: Mutex<Option<Desired>>,
}

impl Handle {
    /// Acknowledge settled recording ports and one complete run. The caller waits outside the
    /// graph lock and never on the audio callback; a runtime that is gone drops the ack.
    pub fn flush(&self) -> std::sync::mpsc::Receiver<Result<(), String>> {
        let (ack, done) = std::sync::mpsc::sync_channel(1);
        let armed = self.last.lock().as_ref()
            .map(|d| d.record.iter().map(|(slot, _)| slot.clone()).collect()).unwrap_or_default();
        let mut mail = self.mail.lock();
        if !mail.closed {
            mail.flush.push(Flush { armed, ack });
        }
        drop(mail);
        let _ = self.bell.ring(0);
        done
    }

    fn send(&self, desired: Desired) {
        *self.last.lock() = Some(desired.clone());
        self.mail.lock().desired = Some(desired);
        let _ = self.bell.ring(0);
    }

    /// Send only what is new, and say whether it did.
    pub fn send_if_changed(&self, desired: Desired) -> bool {
        let fresh = self.last.lock().as_ref() != Some(&desired);
        if fresh {
            self.send(desired);
        }
        fresh
    }

    pub fn request(&self, request: goofi_node::Request) {
        let mut mail = self.mail.lock();
        match request.kind {
            goofi_node::RequestKind::Refresh => mail.refresh.push(request.key),
            goofi_node::RequestKind::Pulse => mail.pulse.push(request.key),
        }
        drop(mail);
        let _ = self.bell.ring(0);
    }

    pub fn stop(&self) {
        self.halt.stop();
        let _ = self.bell.ring(0);
    }
}

/// Stop every handle, then WAIT for each to release its shared memory, under one ceiling.
pub fn stop_all<'a>(handles: impl Iterator<Item = &'a Handle> + Clone) {
    handles.clone().for_each(Handle::stop);
    goofi_transport::wait_released(handles.map(|h| &*h.halt));
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct Spawn {
    /// The engine this node belongs to; the thread wears it, and it is what cuts the recorder's
    /// segment.
    pub engine: &'static str,
    pub uid: Uid,
    /// The instance, which names the recorder's one door.
    pub instance: String,
    pub base: String,
    pub manifest: &'static NodeManifest,
    /// The params as the engine counts them: the consts, the atomics and every binding index.
    pub decls: Vec<ParamDecl>,
    pub params: Arc<[AtomicU64]>,
    pub time: Arc<goofi_core::time::Time>,
}

/// Create the node's services on the caller's thread, where a failure can be reported, and park the
/// runtime on them. `make` builds the executor ON that thread, so it may hold what cannot cross one.
pub fn spawn<E: Executor + 'static>(
    iox: &Iox,
    spawn: Spawn,
    shared: Arc<Shared>,
    bells: &IoxNode,
    make: impl FnOnce() -> E + Send + 'static,
) -> Result<Handle, String> {
    let node = iox.node()?;
    let door = event_service(&node, &door_service(&spawn.base))?;
    let listener = door.listener_builder().create().map_err(|e| format!("listener: {e}"))?;
    let bell = Doorbell::open(bells, &door_service(&spawn.base))?;
    let mut outs = Vec::with_capacity(spawn.manifest.outputs.len());
    for out in spawn.manifest.outputs {
        let service = stream_service(&node, &output_service(&spawn.base, out.name), ServiceKind::Data)?;
        let publisher = publisher(&service, out.name, INITIAL_SLICE)?;
        outs.push(Out_ { service, publisher, bells: Vec::new(), record: None });
    }
    let mail = Arc::new(Mutex::new(Mail::default()));
    let halt = Arc::new(Halt::default());
    let (thread_mail, thread_halt) = (mail.clone(), halt.clone());
    goofi_transport::thread(format!("goofi-{}-{}", spawn.engine, spawn.manifest.type_name))
        .spawn(move || {
            goofi_supervisor::log::set_source(goofi_supervisor::log::Source {
                component: spawn.engine.into(),
                node: Some(spawn.uid.to_hex()),
            });
            // The executor is BUILT in here too: a factory that panics must still release the
            // halt, or the exit waits its whole ceiling on a node that never started.
            let inner = thread_halt.clone();
            inner.wear();
            let posted = thread_mail.clone();
            let run = AssertUnwindSafe(move || {
                let runtime = Runtime {
                    uid: spawn.uid,
                    engine: spawn.engine,
                    base: spawn.base,
                    record_door: record_door_service(&spawn.instance),
                    record_bell: None,
                    held_bells: Vec::new(),
                    manifest: spawn.manifest,
                    decls: spawn.decls,
                    time: spawn.time.clone(),
                    params: spawn.params,
                    consts: Vec::new(),
                    values: Vec::new(),
                    outs,
                    retired: Vec::new(),
                    trouble: None,
                    reopen: None,
                    slots: Vec::new(),
                    binds: Vec::new(),
                    evaluated: IndexMap::new(),
                    errors: IndexMap::new(),
                    bad_inputs: IndexMap::new(),
                    fault: None,
                    stage: NodeStage::Setup,
                    pulsed: Vec::new(),
                    shared,
                    mail: thread_mail,
                    last_tick: Instant::now(),
                    last_run: Instant::now(),
                    last_ufreq: None,
                    listener,
                    exec: make(),
                    node,
                };
                runtime.run(&inner);
            });
            let _ = std::panic::catch_unwind(run);
            *posted.lock() = Mail { closed: true, ..Mail::default() };
            thread_halt.release();
        })
        .map_err(|e| format!("could not start the node's runtime thread: {e}"))?;
    Ok(Handle { mail, halt, bell, last: Mutex::new(None) })
}

/// One output's door out: who drinks from it, who to wake once something is on it, and — while the
/// slot is armed — the recorder's own deep-buffered second publisher.
struct Out_ {
    service: ByteService,
    publisher: BytePublisher,
    bells: Vec<(String, Doorbell, EventId)>,
    /// The armed port and the arming it serves; a new arming replaces it under a new name.
    record: Option<(u64, goofi_transport::RecordPort)>,
}

struct SlotSub {
    inbox: usize,
    wire: usize,
    service: String,
    source: String,
    subscriber: ByteSubscriber,
}

/// Per inbox, the wires it holds in order: what the executor is told.
fn wiring(slots: &[SlotSub]) -> BTreeMap<usize, Vec<(ServiceName, String)>> {
    let mut wiring: BTreeMap<usize, Vec<_>> = BTreeMap::new();
    for s in slots {
        wiring.entry(s.inbox).or_default().push((s.service.clone(), s.source.clone()));
    }
    wiring
}

struct Bind {
    param: usize,
    /// The one dimension this binding writes; `None` writes the whole param.
    elem: Option<usize>,
    key: ParamKey,
    expr: Expression,
    /// What the engine sent, so a re-send that moved nothing is told from one that did.
    sent: (String, Vec<(String, Var)>),
    trigger: bool,
    /// Per stream variable: its name, the service, and this runtime's subscriber on it.
    streams: Vec<(String, String, ByteSubscriber)>,
}

impl Bind {
    /// Whether the pace re-evaluates it: an expression with no stream can still follow the time,
    /// where a bare variable moves only when it is re-sent.
    fn timed(&self) -> bool {
        self.streams.is_empty() && self.expr.id.is_some()
    }
}

struct Runtime<E: Executor> {
    uid: Uid,
    engine: &'static str,
    base: String,
    record_door: ServiceName,
    /// The node's one bell on that door, opened by the first arming and kept for its life.
    record_bell: Option<Arc<Doorbell>>,
    /// One bell per producer door this node asks a held frame of, opened once and kept: a door
    /// opened twice on one node is a second service state.
    held_bells: Vec<(String, Doorbell)>,
    manifest: &'static NodeManifest,
    decls: Vec<ParamDecl>,
    time: Arc<goofi_core::time::Time>,
    params: Arc<[AtomicU64]>,
    consts: Vec<Param>,
    /// The live value per param: the const, or what its binding last evaluated to.
    values: Vec<Param>,
    outs: Vec<Out_>,
    slots: Vec<SlotSub>,
    binds: Vec<Bind>,
    evaluated: IndexMap<ParamKey, Param>,
    errors: IndexMap<ParamKey, String>,
    /// Per Array inbox whose newest frame did not decode: why. Cleared by a frame that does.
    bad_inputs: IndexMap<usize, String>,
    /// The fault last reported: the executor's, else the first bad inbox, else the recording's.
    fault: Option<NodeFault>,
    stage: NodeStage,
    /// The params a pulse raised, each lowered once a tick has passed since its raise.
    pulsed: Vec<(usize, Instant)>,
    shared: Arc<Shared>,
    mail: Arc<Mutex<Mail>>,
    last_tick: Instant,
    last_run: Instant,
    /// When the rate was last REPORTED, which is not when it was last measured.
    last_ufreq: Option<Instant>,
    listener: Listener,
    exec: E,
    /// Ports a newer arming replaced, each kept until the recorder has let go of it.
    retired: Vec<goofi_transport::RecordPort>,
    /// What the recording last cost and why: the drops, and the cause the node wears as a fault.
    trouble: Option<(u64, String)>,
    /// What the engine last wanted, kept while a port of it failed to open: the next tick applies
    /// it again, so a service that was not there yet is reached when it is.
    reopen: Option<Desired>,
    /// Last: every port above is built from it, and fields drop in declaration order.
    node: IoxNode,
}

/// A runtime's trouble, logged under its engine; the log groups a repeat.
fn trouble(engine: &str, text: &str) {
    goofi_supervisor::log::record(goofi_supervisor::log::Source::component("runtime"), goofi_supervisor::log::Level::Error, None, format!("{engine}: {text}"));
}

impl<E: Executor> Runtime<E> {
    fn run(mut self, halt: &Halt) {
        while !halt.stopped() {
            self.park();
            if halt.stopped() {
                break;
            }
            let params = &self.params;
            self.pulsed.retain(|(i, raised)| {
                let held = raised.elapsed() < TICK;
                if !held {
                    params[*i].store(0.0f64.to_bits(), Ordering::Relaxed);
                }
                held
            });
            let mail = std::mem::take(&mut *self.mail.lock());
            if let Some(d) = mail.desired.or_else(|| self.reopen.take()) {
                self.apply(d);
            }
            for key in mail.refresh {
                let options = self.index_of(&key).and_then(|i| self.exec.refresh(i));
                self.shared.report(self.uid, Status::RefreshOptions { key, options });
            }
            for key in &mail.pulse {
                if let Some(i) = self.index_of(key) {
                    self.raise(i);
                }
            }
            self.receive();
            self.retired.retain(|port| !port.spent());
            let now = Instant::now();
            let due = !mail.flush.is_empty() || self.exec.next_wake(self.last_run).is_some_and(|t| t <= now);
            if due || self.last_tick + TICK <= now {
                self.last_tick = now;
                let mut pass = Pass::default();
                for i in 0..self.binds.len() {
                    if self.binds[i].timed() {
                        self.evaluate(i, &mut pass);
                    }
                }
                self.report(pass);
                self.sync(false);
            }
            if due {
                self.last_run = now;
                self.tick();
            }
            for Flush { armed, ack } in mail.flush {
                let missing: Vec<_> = armed.iter().filter(|name| {
                    !self.manifest.outputs.iter().zip(&self.outs).any(|(decl, out)| {
                        decl.name == name.as_str() && out.record.as_ref().is_some_and(|(_, p)| !p.retired())
                    })
                }).cloned().collect();
                let result = if missing.is_empty() { Ok(()) } else {
                    Err(format!("recording ports are not ready: {}", missing.join(", ")))
                };
                let _ = ack.try_send(result);
            }
            self.wear();
        }
    }

    /// Park until something rings or a duty is due: the executor's run, or a paced one of this
    /// runtime's own. With neither, park until a ring — the halt's included.
    fn park(&mut self) {
        let paced = !self.pulsed.is_empty() || self.binds.iter().any(Bind::timed);
        let own = paced.then(|| self.last_tick + TICK);
        let due = match (self.exec.next_wake(self.last_run), own) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        match due {
            None => {
                let _ = self.listener.blocking_wait_all(|_| {});
            }
            Some(at) => {
                let wait = at.saturating_duration_since(Instant::now());
                // A due run takes what already rang; a wait still ahead is floored by the transport.
                if wait.is_zero() {
                    let _ = self.listener.try_wait_all(|_| {});
                } else {
                    goofi_transport::wait_within(&self.listener, wait, |_| {});
                }
            }
        }
    }

    fn apply(&mut self, d: Desired) {
        let wanted = d.clone();
        self.consts = d.consts;
        let (slots, binds): (Vec<Sub>, Vec<Sub>) = d.subs.into_iter().partition(|s| matches!(s, Sub::Slot { .. }));
        let opened = self.apply_slots(slots) & self.apply_bells(d.targets);
        self.reopen = (!opened).then_some(wanted);
        let trigger = self.apply_binds(binds);
        self.apply_records(&d.record);
        // Every literal goes in first, then each binding writes what it owns over it: a whole
        // param, or one dimension of it, which leaves the literal in the dimensions nobody drives.
        for (i, c) in self.consts.iter().enumerate() {
            let raised = self.pulsed.iter().any(|(p, _)| *p == i);
            if !raised {
                store(&self.params, &self.decls, i, c);
            }
        }
        let mut pass = Pass::default();
        for i in 0..self.binds.len() {
            self.evaluate(i, &mut pass);
        }
        self.report(pass);
        self.sync(trigger);
    }

    /// Answers whether every wire opened; one that did not is logged and tried again next tick.
    /// An inbox whose wire set moved is told the whole set.
    fn apply_slots(&mut self, subs: Vec<Sub>) -> bool {
        let before = wiring(&self.slots);
        let mut old = std::mem::take(&mut self.slots);
        let mut opened = true;
        for sub in subs {
            let Sub::Slot { inbox, wire, service, source } = sub else { continue };
            let kept = take_where(&mut old, |s| s.service == service).map(|s| Ok(s.subscriber));
            match kept.unwrap_or_else(|| open_output_subscriber(&self.node, &service)) {
                Ok(subscriber) => self.slots.push(SlotSub { inbox, wire, service, source, subscriber }),
                Err(e) => {
                    trouble(self.engine, &format!("wire `{service}` did not open: {e}"));
                    opened = false;
                }
            }
        }
        self.slots.sort_by_key(|s| (s.inbox, s.wire));
        let after = wiring(&self.slots);
        for inbox in before.keys().chain(after.keys()).collect::<std::collections::BTreeSet<_>>() {
            if before.get(inbox) != after.get(inbox) {
                self.exec.rewire(*inbox, after.get(inbox).map_or(&[], Vec::as_slice));
            }
        }
        opened
    }

    /// Answers whether a binding re-sent with a resolved value asks for a run: a variables edit
    /// reaching a triggering binding is an arrival, where binding a bare reference only subscribes.
    fn apply_binds(&mut self, subs: Vec<Sub>) -> bool {
        let mut old = std::mem::take(&mut self.binds);
        let mut triggering = false;
        for sub in subs {
            let Sub::Bind { param, elem, key, source, id, vars, trigger } = sub else { continue };
            let mut previous = take_where(&mut old, |b| b.key == key);
            let moved = previous.as_ref().is_none_or(|p| p.sent.0 != source || p.sent.1 != vars);
            if moved && trigger && key.group != COMMON && vars.iter().any(|(_, v)| matches!(v, Var::Value(_))) {
                triggering = true;
            }
            let sent = (source.clone(), vars.clone());
            let mut streams = Vec::new();
            let mut kept_names = Vec::new();
            let mut resolved = Vec::with_capacity(vars.len());
            for (var, v) in vars {
                let v = match v {
                    Var::Stream { service, held } => {
                        let kept = previous
                            .as_mut()
                            .and_then(|p| take_where(&mut p.streams, |(n, s, _)| *n == var && *s == service));
                        if kept.is_some() {
                            kept_names.push(var.clone());
                        }
                        let fresh = kept.is_none();
                        match kept.map(|(_, _, s)| Ok(s)).unwrap_or_else(|| open_output_subscriber(&self.node, &service)) {
                            Ok(subscriber) => {
                                streams.push((var.clone(), service.clone(), subscriber));
                                // Subscribed: a producer holding its last frame is asked for it now.
                                if let (true, Some(door)) = (fresh, &held) {
                                    self.ring_held(door);
                                }
                                Var::Stream { service, held }
                            }
                            Err(e) => Var::Missing(e),
                        }
                    }
                    other => other,
                };
                resolved.push((var, v));
            }
            let mut expr = Expression::new(source, id, resolved);
            if let Some(p) = &previous {
                expr.carry(&p.expr, |name| kept_names.iter().any(|n| n == name));
            }
            self.binds.push(Bind { param,
                elem, key, expr, sent, trigger, streams });
        }
        let mut pass = Pass::default();
        for dropped in old {
            pass.values |= self.evaluated.shift_remove(&dropped.key).is_some();
            if self.errors.shift_remove(&dropped.key).is_some() {
                pass.errors.push((dropped.key, None));
            }
        }
        self.report(pass);
        triggering
    }

    /// Ring a producer's door for the frame it holds, through this node's one bell on it.
    fn ring_held(&mut self, door: &str) {
        if !self.held_bells.iter().any(|(d, _)| d == door) {
            match Doorbell::open(&self.node, door) {
                Ok(bell) => self.held_bells.push((door.to_string(), bell)),
                Err(e) => return trouble(self.engine, &format!("bell onto `{door}` did not open: {e}")),
            }
        }
        if let Some((_, bell)) = self.held_bells.iter().find(|(d, _)| d == door) {
            let _ = bell.ring(0);
        }
    }

    /// Answers whether every bell opened; one that did not is logged and tried again next tick. A
    /// bell just opened rings once: a frame published before it hung is on the wire unannounced.
    fn apply_bells(&mut self, targets: Vec<Vec<(String, EventId)>>) -> bool {
        let mut opened = true;
        for (out, targets) in self.outs.iter_mut().zip(targets) {
            let mut old = std::mem::take(&mut out.bells);
            for (door, id) in targets {
                match take_where(&mut old, |(d, _, _)| *d == door) {
                    Some((_, bell, _)) => out.bells.push((door, bell, id)),
                    None => match Doorbell::open(&self.node, &door) {
                        Ok(bell) => {
                            let _ = bell.ring(id);
                            out.bells.push((door, bell, id));
                        }
                        Err(e) => {
                            trouble(self.engine, &format!("bell onto `{door}` did not open: {e}"));
                            opened = false;
                        }
                    },
                }
            }
        }
        opened
    }

    /// The recorder's second publisher on every armed output: opened by arming, because a segment
    /// is a whole budget, and released by the recorder's read, so delivered frames stay.
    fn apply_records(&mut self, armed: &[(String, u64)]) {
        let shape = record_shape(self.engine);
        let mut failed = Vec::new();
        if !armed.is_empty() && self.record_bell.is_none() {
            match Doorbell::open(&self.node, &self.record_door) {
                Ok(bell) => self.record_bell = Some(Arc::new(bell)),
                Err(e) => failed.push(format!("could not reach the recorder's door: {e}")),
            }
        }
        for (out, decl) in self.outs.iter_mut().zip(self.manifest.outputs) {
            let wanted = armed.iter().find(|(slot, _)| slot == decl.name).map(|(_, serial)| *serial);
            match (out.record.as_mut(), wanted) {
                (Some((held, port)), Some(serial)) if *held == serial => {
                    port.armed();
                    continue;
                }
                // A new arming is a new service: the old port keeps the name the recorder may
                // already have let go of, and is dropped once nobody reads it.
                (Some(_), Some(_)) => {
                    let (_, mut port) = out.record.take().expect("matched above");
                    port.retire();
                    self.retired.push(port);
                }
                (Some((_, port)), None) => {
                    port.retire();
                    if port.spent() {
                        out.record = None;
                    }
                    continue;
                }
                (None, None) => continue,
                (None, Some(_)) => {}
            }
            let serial = wanted.expect("armed above");
            let Some(bell) = &self.record_bell else { continue };
            let opened = goofi_transport::RecordPort::open(
                &self.node,
                &record_service(&self.base, decl.name, serial),
                bell,
                decl.name,
                shape,
            );
            match opened {
                Ok(port) => out.record = Some((serial, port)),
                Err(e) => failed.push(format!("could not arm `{}`: {e}", decl.name)),
            }
        }
        self.trouble = (!failed.is_empty()).then(|| (0, failed.join("; ")));
    }

    fn index_of(&self, key: &ParamKey) -> Option<usize> {
        self.decls.iter().position(|d| d.group == key.group && d.name == key.name)
    }

    /// Record or clear a param's error, keeping only what CHANGED: the graph files the delta
    /// against the instance.
    fn record_error(&mut self, key: ParamKey, error: Option<String>, pass: &mut Pass) {
        let changed = match &error {
            Some(e) => self.errors.insert(key.clone(), e.clone()).as_ref() != Some(e),
            None => self.errors.shift_remove(&key).is_some(),
        };
        if changed {
            pass.errors.push((key, error));
        }
    }

    /// What a pass of evaluations changed, said ONCE — a batch yields at most one decision, and
    /// evaluating a node's every binding is one batch.
    fn report(&self, pass: Pass) {
        if pass.values {
            let evaluated = self.evaluated.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            self.shared.report(self.uid, Status::ParamValues { evaluated });
        }
        if !pass.errors.is_empty() {
            self.shared.report(self.uid, Status::BindingErrors { errors: pass.errors });
        }
    }

    /// File what a call into the executor changed.
    fn absorb(&mut self, ticked: Ticked) {
        let mut pass = Pass::default();
        for (key, error) in ticked.errors {
            self.record_error(key, error, &mut pass);
        }
        self.report(pass);
        if ticked.replan {
            self.shared.ask_settle();
        }
        if let Some(hz) = ticked.ufreq {
            let now = Instant::now();
            if self.last_ufreq.is_none_or(|t| now - t >= UFREQ_REPORT) {
                self.last_ufreq = Some(now);
                self.shared.report(self.uid, Status::Ufreq { hz });
            }
        }
    }

    /// The live values, and the executor told when they moved — or when an arrival asks for a run.
    fn sync(&mut self, trigger: bool) {
        let values: Vec<Param> = self.consts.iter().enumerate().map(|(i, value)| {
            let whole = self.binds.iter().find(|b| b.param == i && b.elem.is_none()).and_then(|b| self.evaluated.get(&b.key));
            let held = whole.unwrap_or(value).clone();
            // An element binding leaves its value in the cells, so a driven vector reads off them.
            if self.binds.iter().any(|b| b.param == i && b.elem.is_some()) {
                goofi_core::control::read(&goofi_core::Data::numbers(dims(&self.params, &self.decls, i)), &held)
            } else {
                held
            }
        }).collect();
        if values != self.values || trigger {
            self.values = values;
            let ticked = self.exec.params_changed(&self.values, trigger);
            self.absorb(ticked);
        }
    }

    /// Every frame that arrived, in order into the executor — or the newest alone, where the
    /// executor says only that one matters — and every binding a frame reached is evaluated once.
    fn receive(&mut self) {
        let mut moved = false;
        let latest_only = self.exec.latest_only();
        let (exec, bad) = (&mut self.exec, &mut self.bad_inputs);
        let mut arrived = |inbox: usize, wire: usize, payload: &[u8]| match goofi_codec::decode(payload) {
            Ok(frame) => {
                bad.shift_remove(&inbox);
                moved |= exec.arrive(inbox, wire, &frame);
            }
            Err(why) => {
                bad.insert(inbox, why);
            }
        };
        for s in &self.slots {
            // Taking a sample is free where decoding it is not, and only the last one survives:
            // a producer running flat out otherwise outruns this thread out of its own tick.
            if latest_only {
                let mut newest = None;
                while let Ok(Some(sample)) = s.subscriber.receive() {
                    newest = Some(sample);
                }
                if let Some(sample) = newest {
                    arrived(s.inbox, s.wire, sample.payload());
                }
                continue;
            }
            while let Ok(Some(sample)) = s.subscriber.receive() {
                arrived(s.inbox, s.wire, sample.payload());
            }
        }
        if moved {
            self.shared.ask_settle();
        }
        let mut touched = Vec::new();
        let mut undecodable = Vec::new();
        for (i, b) in self.binds.iter_mut().enumerate() {
            for (var, _, subscriber) in &b.streams {
                let mut newest = None;
                while let Ok(Some(sample)) = subscriber.receive() {
                    newest = Some(goofi_codec::decode(sample.payload()));
                }
                match newest {
                    Some(Ok(frame)) => {
                        b.expr.deliver(var, frame);
                        touched.push(i);
                    }
                    Some(Err(why)) => undecodable.push((b.key.clone(), format!("`{var}` does not decode: {why}"))),
                    None => {}
                }
            }
        }
        touched.dedup();
        let trigger = touched.iter().any(|i| self.binds[*i].trigger && self.binds[*i].key.group != COMMON);
        let mut pass = Pass::default();
        for i in touched {
            self.evaluate(i, &mut pass);
        }
        for (key, why) in undecodable {
            self.record_error(key, Some(why), &mut pass);
        }
        self.report(pass);
        if trigger || moved || !self.binds.is_empty() {
            self.sync(trigger);
        }
    }

    /// Report the stage and the fault the node wears, when either moved. The fault is the
    /// executor's, else the first bad inbox, else what the recording costs.
    fn wear(&mut self) {
        let stage = self.exec.stage();
        if stage != self.stage {
            self.stage = stage;
            self.shared.report(self.uid, Status::Stage { stage });
        }
        let since = self.time.now();
        let fault = match self.exec.fault() {
            Some(Fault::Setup(msg)) => Some(NodeFault::Setup { msg, since }),
            Some(Fault::Process(msg)) => Some(NodeFault::Process { msg, since }),
            None => self
                .bad_inputs
                .first()
                .map(|(inbox, why)| format!("input {inbox} does not decode: {why}"))
                .or_else(|| {
                    self.trouble.as_ref().map(|(dropped, why)| match dropped {
                        0 => format!("recording: {why}"),
                        n => format!("recording dropped {n} frames: {why}"),
                    })
                })
                .map(|msg| NodeFault::Process { msg, since }),
        };
        let same = match (&self.fault, &fault) {
            (Some(a), Some(b)) => std::mem::discriminant(a) == std::mem::discriminant(b) && a.msg() == b.msg(),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.fault = fault;
            self.shared.report(self.uid, Status::Fault { fault: self.fault.clone() });
        }
    }

    /// Why a recording loan was refused: an outsized frame, or shared memory that ran out.
    fn loan_refused(engine: &str, len: usize) -> String {
        let ceiling = record_shape(engine).slice;
        match len > ceiling {
            true => format!("a {len} byte frame is over the {ceiling} byte ceiling"),
            false => "no shared memory left".to_string(),
        }
    }

    /// The run: the executor says what goes out on each output, onto the wire and its recording.
    fn tick(&mut self) {
        let readers: Vec<bool> = self.outs.iter().map(|o| goofi_transport::subscribers(&o.service) > 0).collect();
        let recorded: Vec<bool> = self.outs.iter().map(|o| o.record.as_ref().is_some_and(|(_, r)| !r.retired())).collect();
        let cx = Cx {
            values: &self.values,
            consts: &self.consts,
            params: &self.params,
            readers: &readers,
            recorded: &recorded,
            now: self.time.now(),
            time: &self.time,
            uid: self.uid,
        };
        let (outs, engine, trouble_) = (&mut self.outs, self.engine, &mut self.trouble);
        // A retired port is dropped HERE rather than at the disarm, so the release follows the
        // recorder's own reading and never outruns it.
        for out in outs.iter_mut() {
            if out.record.as_ref().is_some_and(|(_, p)| p.spent()) {
                out.record = None;
            }
        }
        let ticked = self.exec.run(&cx, &mut |i, out| {
            let Some(port) = outs.get(i) else { return };
            let encoded;
            let (wire, record) = match out {
                Out::Bytes(bytes) => (Some(bytes), None),
                Out::Record(bytes) => (None, Some(bytes)),
                Out::Frame(frame) => match goofi_codec::encode(frame) {
                    Ok(bytes) => {
                        encoded = bytes;
                        (Some(&encoded[..]), Some(&encoded[..]))
                    }
                    Err(e) => {
                        trouble(engine, &format!("output {i} cannot cross: {e}"));
                        return;
                    }
                },
            };
            if let Some(bytes) = wire {
                if let Err(e) = goofi_transport::publish(&port.publisher, bytes, port.bells.iter().map(|(_, bell, id)| (bell, *id))) {
                    trouble(engine, &format!("output {i} was not published: {e}"));
                }
            }
            if let (Some(bytes), Some((_, rec))) = (record, port.record.as_ref()) {
                let ok = rec.send(bytes.len(), |loan| goofi_transport::write_parts(loan, [bytes]));
                if !ok && !rec.retired() {
                    let (dropped, _) = trouble_.get_or_insert_with(|| (0, Self::loan_refused(engine, bytes.len())));
                    *dropped += 1;
                }
            }
        });
        self.absorb(ticked);
    }

    /// Raise a pulse param for one tick, and tell the executor.
    fn raise(&mut self, i: usize) {
        self.params[i].store(1.0f64.to_bits(), Ordering::Relaxed);
        self.pulsed.push((i, Instant::now()));
        let ticked = self.exec.pulse(i);
        self.absorb(ticked);
    }

    /// One binding's value into its atomic — the literal when nothing has arrived or it cannot be
    /// evaluated — and the report of what changed.
    fn evaluate(&mut self, i: usize, pass: &mut Pass) {
        let b = &self.binds[i];
        let param = b.param;
        let elem = b.elem;
        // An element binding is evaluated against that one dimension as a scalar of the param's kind.
        let target = &match elem {
            Some(k) => self.consts[param].dim(k),
            None => self.consts[param].clone(),
        };
        let evaluator = self.shared.evaluator.lock().clone();
        let t = self.time.now();
        let (value, error) = match b.expr.evaluate(evaluator.as_deref(), t, target) {
            Ok(Some(v)) if !scalar(&v).is_finite() => (None, Some(format!("evaluated to {}", scalar(&v)))),
            Ok(v) => (v, None),
            Err(e) => (None, Some(e)),
        };
        let key = b.key.clone();
        // A source on a pulse is a gate: the RISE is the request, and an unevaluated pass keeps
        // the edge memory.
        if matches!(target, Param::Pulse) {
            if let Some(level) = value {
                let was_high = self.evaluated.insert(key.clone(), level.clone()).and_then(|p| p.as_bool()).unwrap_or(false);
                if !was_high && level.as_bool() == Some(true) {
                    self.raise(param);
                }
            }
            self.record_error(key, error, pass);
            return;
        }
        let held = value.as_ref().unwrap_or(target);
        match elem {
            Some(k) => store_dim(&self.params, &self.decls, param, k, scalar(held)),
            None => {
                // A whole write spares the dimensions an element binding of its own drives.
                let owned: Vec<usize> = self.binds.iter().filter(|o| o.param == param).filter_map(|o| o.elem).collect();
                let mut merged = held.clone();
                if !owned.is_empty() {
                    let mut dims = dims(&self.params, &self.decls, param);
                    for (k, v) in held.as_vec().unwrap_or_default().iter().enumerate() {
                        if !owned.contains(&k) && k < dims.len() {
                            dims[k] = *v;
                        }
                    }
                    merged = goofi_core::control::read(&goofi_core::Data::numbers(dims), held);
                }
                store(&self.params, &self.decls, param, &merged);
            }
        }
        pass.values |= match value {
            Some(v) => self.evaluated.insert(key.clone(), v.clone()).as_ref() != Some(&v),
            None => self.evaluated.shift_remove(&key).is_some(),
        };
        self.record_error(key, error, pass);
    }
}

/// What one pass of binding evaluations changed. The values ride as the WHOLE sparse map, which
/// the graph's copy is replaced with, so a value it is no longer told does not stay.
#[derive(Default)]
struct Pass {
    values: bool,
    errors: Vec<(ParamKey, Option<String>)>,
}

/// A param's scalar as an engine reads it: a number as itself, a bool as 0/1, an option as its
/// index, free text as 0, and a pulse — which holds no value — as 0.
pub fn scalar(p: &Param) -> f64 {
    p.as_f64().unwrap_or_else(|| match p {
        Param::Str { value, options: Some(options), .. } => {
            options.iter().position(|o| o == value).map_or(0.0, |i| i as f64)
        }
        _ => 0.0,
    })
}

/// The record's value for one declared param, the declared default where the record has none.
pub fn param_of(params: &ParamGroups, d: &ParamDecl) -> Param {
    goofi_node::param(params, d.group, d.name).cloned().unwrap_or_else(|| d.spec.to_param())
}

pub fn scalar_of(params: &ParamGroups, d: &ParamDecl) -> f64 {
    scalar(&param_of(params, d))
}

/// How many dimensions a declared param has.
pub fn dims_of(d: &ParamDecl) -> usize {
    match d.spec {
        goofi_node::ParamSpec::Num { default, .. } => default.len().max(1),
        _ => 1,
    }
}

/// A node's param cells: one per param, then every dimension past the first of each vector in
/// order — so a param's index is its cell, and a vector's tail is where [`tail_of`] says.
pub fn cells_of(params: &ParamGroups, decls: &[ParamDecl]) -> Vec<AtomicU64> {
    let mut cells: Vec<AtomicU64> = decls.iter().map(|d| AtomicU64::new(scalar_of(params, d).to_bits())).collect();
    for d in decls {
        let p = param_of(params, d);
        let value = p.as_vec().unwrap_or(&[]);
        cells.extend((1..dims_of(d)).map(|k| AtomicU64::new(value.get(k).copied().unwrap_or(0.0).to_bits())));
    }
    cells
}

/// The cell of param `i`'s second dimension; the rest follow it. The tails sit at the END of the
/// cells, so a reader holding only a prefix of the decls — an engine's own params without the
/// universal ones behind them, which are scalars — finds them all the same.
pub fn tail_of(cells: usize, decls: &[ParamDecl], i: usize) -> usize {
    let tails: usize = decls.iter().map(|d| dims_of(d) - 1).sum();
    cells - tails + decls[..i].iter().map(|d| dims_of(d) - 1).sum::<usize>()
}

/// Every dimension of param `i` as the cells hold it.
pub fn dims(cells: &[AtomicU64], decls: &[ParamDecl], i: usize) -> Vec<f64> {
    let tail = tail_of(cells.len(), decls, i);
    let at = |cell: usize| cells.get(cell).map_or(0.0, |a| f64::from_bits(a.load(Ordering::Relaxed)));
    std::iter::once(at(i)).chain((1..dims_of(&decls[i])).map(|k| at(tail + k - 1))).collect()
}

/// Param `i` into its cells, every dimension.
pub fn store(cells: &[AtomicU64], decls: &[ParamDecl], i: usize, p: &Param) {
    cells[i].store(scalar(p).to_bits(), Ordering::Relaxed);
    let tail = tail_of(cells.len(), decls, i);
    let value = p.as_vec().unwrap_or(&[]);
    for k in 1..dims_of(&decls[i]) {
        cells[tail + k - 1].store(value.get(k).copied().unwrap_or(0.0).to_bits(), Ordering::Relaxed);
    }
}

/// One dimension of param `i` into its cell, the rest left as they are.
pub fn store_dim(cells: &[AtomicU64], decls: &[ParamDecl], i: usize, k: usize, v: f64) {
    if k >= dims_of(&decls[i]) {
        return;
    }
    let cell = if k == 0 { i } else { tail_of(cells.len(), decls, i) + k - 1 };
    if let Some(c) = cells.get(cell) {
        c.store(v.to_bits(), Ordering::Relaxed);
    }
}

/// A `Str` param's text; any other kind has none, and nor has a param whose first desired state
/// has not arrived yet, since a runtime ticks before its consts arrive.
pub fn text(consts: &[Param], param: usize) -> String {
    match consts.get(param) {
        Some(Param::Str { value, .. }) => value.clone(),
        _ => String::new(),
    }
}

pub fn flag(consts: &[Param], param: usize) -> bool {
    consts.get(param).and_then(|p| p.as_bool()).unwrap_or(false)
}
