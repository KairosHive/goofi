//! The machines' own thread, `goofi-machines`: it wakes for a reason alone — a landing, a decision's
//! due, a stamped request, a replaced model, a write to a variable a machine reads, or a frame of a
//! node one reads — settles every instant owed and writes each playhead's group through the store,
//! as the follower does: no graph lock, no undo entry, no dirty mark. The graph lock is for config
//! edits, which hand it a replaced model with the node reads resolved.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use goofi_core::indexmap::IndexMap;
use goofi_core::{Data, Param};
use goofi_graph::expr_rewrite::{self, Target};
use goofi_graph::machine::{Machine, Machines};
use goofi_graph::Graph;
use goofi_node::{ExprEvaluator, Local};
use goofi_transport::{machines_door, output_service, service_base, Door, Hub, Payload, Subscriber, MACHINES_FRAME_ID, MACHINES_INBOX_ID};

use crate::AppState;

/// What the graph resolved the machines' `nd()` terms to: the service of each output read, the
/// value of each param read, or why the term names nothing — resolved at the settle, as a param's.
#[derive(Default)]
pub struct NodeReads {
    pub streams: HashMap<(String, Option<String>), Result<String, String>>,
    pub params: HashMap<(String, String, String), Result<Param, String>>,
}

pub enum Msg {
    Configure(IndexMap<String, Machine>, Option<Arc<dyn ExprEvaluator>>, NodeReads),
    /// An effect, stamped with the patch second of the op that asked for it.
    Fire { at: f64, machine: String, playhead: String, transition: String },
    Jump { at: f64, machine: String, playhead: String, state: String },
    Reset { at: f64, machine: Option<String> },
    /// A variable was written; one a machine reads is a reason to look again.
    Changed(String),
}

/// The manager's hand on the thread: a message in its inbox, and a ring on its door for it.
pub struct Driver {
    tx: Sender<Msg>,
    door: Arc<Door>,
}

impl Driver {
    pub fn new(hub: &Hub, instance: &str) -> (Driver, Receiver<Msg>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (Driver { tx, door: hub.door(&machines_door(instance)) }, rx)
    }

    /// Hand the thread the settled model, with every node read resolved against the graph.
    pub fn configure(&self, g: &Graph) {
        self.send(Msg::Configure(g.machines().clone(), g.evaluator(), node_reads(g)));
    }

    pub fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
        self.door.ring(MACHINES_INBOX_ID);
    }

    /// The store's word that a variable moved, for the thread's inbox.
    pub fn watch(&self) -> Arc<dyn Fn(&str) + Send + Sync> {
        let tx = self.tx.clone();
        let door = self.door.clone();
        Arc::new(move |name: &str| {
            let _ = tx.send(Msg::Changed(name.to_string()));
            door.ring(MACHINES_INBOX_ID);
        })
    }
}

/// Every `nd()` term of the machines' expressions, resolved as a param's binding resolves it.
fn node_reads(g: &Graph) -> NodeReads {
    let mut out = NodeReads::default();
    for m in g.machines().values() {
        for text in m.expressions() {
            let Ok((_, refs)) = expr_rewrite::rewrite_machine(text) else { continue };
            for r in refs {
                match r.target {
                    Target::Node { name, slot } => {
                        let service = g.machine_stream(&name, slot.as_deref()).map(|(uid, s)| output_service(&service_base(g.instance(), uid, g.node_generation(uid)), s));
                        out.streams.insert((name, slot), service);
                    }
                    Target::NodeParam { name, group, param } => {
                        let value = g.machine_param(&name, &group, &param);
                        out.params.insert((name, group, param), value);
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

/// How long the thread sleeps between looks at its inbox while nothing is owed.
const IDLE: Duration = Duration::from_millis(500);

/// The node outputs the thread holds: a subscription per service and the newest frame it carried.
#[derive(Default)]
struct Streams {
    subs: HashMap<String, (Subscriber, Option<Data>)>,
}

impl Streams {
    /// Hold exactly the services `reads` resolved, keeping the frames of those held already.
    fn follow(&mut self, hub: &Hub, reads: &NodeReads) {
        let wanted: HashSet<&String> = reads.streams.values().filter_map(|r| r.as_ref().ok()).collect();
        self.subs.retain(|service, _| wanted.contains(service));
        for service in wanted {
            self.subs.entry(service.clone()).or_insert_with(|| (hub.subscribe(service), None));
        }
    }

    /// Take the newest frame off every subscription.
    fn drain(&mut self) {
        for (sub, held) in self.subs.values_mut() {
            while let Some(payload) = sub.receive() {
                if let Payload::Data(d) = payload {
                    *held = Some(d);
                }
            }
        }
    }

    fn held(&self, service: &str) -> Option<Data> {
        self.subs.get(service).and_then(|(_, held)| held.clone())
    }
}

pub fn spawn(state: AppState, rx: Receiver<Msg>) {
    let owner = state.clone();
    owner.scope.spawn("goofi-machines", move || {
        let (store, time, door) = {
            let g = state.graph.lock();
            (g.variable_store(), g.time(), state.hub.door(&machines_door(g.instance())))
        };
        let mut machines = Machines::default();
        let mut reads = NodeReads::default();
        let mut streams = Streams::default();
        let mut logged: HashSet<String> = HashSet::new();
        loop {
            // Park toward the next instant owed; a ring — a frame read, or the inbox — is the other
            // reason to wake.
            let wait = match machines.next_deadline() {
                Some(at) => goofi_supervisor::timing::Deadline::at(time.instant_at(at)).left().min(IDLE),
                None => IDLE,
            };
            let mut woken = door.wait_until(Instant::now() + wait).has(MACHINES_FRAME_ID);
            loop {
                match rx.try_recv() {
                    Ok(msg) => woken |= take(&mut machines, &mut reads, &mut streams, &state.hub, msg),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
            if state.stopping.stopped() {
                return;
            }
            if machines.is_empty() {
                continue;
            }
            let now = time.now();
            if !woken && machines.next_deadline().is_none_or(|at| at > now) {
                continue;
            }
            // The frames the expressions read, taken in one look; the step runs off every lock.
            let frames: IndexMap<String, Data> = {
                let store = store.lock();
                machines.keys().iter().filter_map(|k| Some((k.clone(), store.get(k)?.clone()))).collect()
            };
            streams.drain();
            let read = |target: &Target| -> Result<Local, String> {
                match target {
                    Target::Variable { key } => frames.get(key).cloned().map(Local::Frame).ok_or_else(|| format!("variable `{key}` is not defined")),
                    Target::Node { name, slot } => {
                        let spelled = match slot {
                            Some(s) => format!("nd('{name}').out.{s}"),
                            None => format!("nd('{name}')"),
                        };
                        let service = reads.streams.get(&(name.clone(), slot.clone())).ok_or_else(|| format!("`{spelled}` is not resolved"))?.as_ref().map_err(Clone::clone)?;
                        streams.held(service).map(Local::Frame).ok_or_else(|| format!("`{spelled}` has not produced a frame yet"))
                    }
                    Target::NodeParam { name, group, param } => reads
                        .params
                        .get(&(name.clone(), group.clone(), param.clone()))
                        .cloned()
                        .ok_or_else(|| format!("`nd('{name}').params.{group}.{param}` is not resolved"))?
                        .map(Local::Value),
                    _ => Err("a machine's own reference".to_string()),
                }
            };
            let writes = machines.advance(now, &read);
            {
                let mut store = store.lock();
                for (name, value) in writes {
                    store.drive(&name, value);
                }
            }
            for why in machines.take_errors() {
                if logged.insert(why.clone()) {
                    goofi_supervisor::log::record(goofi_supervisor::log::Source::component("machines"), goofi_supervisor::log::Level::Error, None, why);
                }
            }
        }
    });
}

/// One message into the machines; whether it is a reason to settle now.
fn take(machines: &mut Machines, reads: &mut NodeReads, streams: &mut Streams, hub: &Hub, msg: Msg) -> bool {
    match msg {
        Msg::Configure(config, evaluator, node_reads) => {
            machines.configure(config, evaluator);
            streams.follow(hub, &node_reads);
            *reads = node_reads;
        }
        Msg::Fire { at, machine, playhead, transition } => machines.fire(at, &machine, &playhead, &transition),
        Msg::Jump { at, machine, playhead, state } => machines.jump(at, &machine, &playhead, &state),
        Msg::Reset { at, machine } => machines.reset(at, machine.as_deref()),
        Msg::Changed(name) => return machines.keys().contains(&name),
    }
    true
}
