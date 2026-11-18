//! The thread of what steps on the clock's position, the machines and the timeline,
//! `goofi-machines`: it wakes for a reason alone — a landing, a decision's
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
use goofi_graph::expr_rewrite::Target;
use goofi_graph::machine::{Machine, Machines};
use goofi_graph::timeline::{Lanes, Take, Track};
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
    Configure(IndexMap<String, Machine>, (IndexMap<String, Track>, f64), Option<Arc<dyn ExprEvaluator>>, NodeReads),
    /// An effect, stamped with the patch second of the op that asked for it.
    Fire { at: f64, machine: String, playhead: String, transition: String },
    Jump { at: f64, machine: String, playhead: String, state: String },
    Reset { at: f64, machine: Option<String> },
    /// A variable was written; one a machine reads is a reason to look again.
    Changed(String),
    /// The clock's position was put somewhere: every run starts over there.
    Jumped,
    /// The section the clock loops in moved: what the thread waits for is another instant.
    Section,
    /// Begin a recording into the track of this id.
    Record(String),
    /// End the recording into the track of this id, and answer what it took.
    Recorded(String, Sender<Take>),
}

/// The manager's hand on the thread: a message in its inbox, and a ring on its door for it.
pub struct Driver {
    tx: Sender<Msg>,
    door: Arc<Door>,
    /// The note tracks as the audio engine's MIDI ports; on a demo, which has none, nobody's.
    ports: goofi_audio::TrackPorts,
}

impl Driver {
    pub fn new(hub: &Hub, g: &mut Graph) -> (Driver, Receiver<Msg>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ports = crate::try_audio_engine(g).map(|audio| audio.track_ports()).unwrap_or_default();
        (Driver { tx, door: hub.door(&machines_door(g.instance())), ports }, rx)
    }

    /// Hand the thread the settled model: the machines, with every node read resolved against
    /// the graph, and the tracks with the clock's tempo. The note tracks are the ports there are.
    pub fn configure(&self, g: &Graph) {
        self.ports.list(g.timeline().values().filter(|t| matches!(t.lanes, Lanes::Note { .. })).map(|t| t.name.clone()).collect());
        let bpm = g.variables().get("clock.bpm").map_or(120.0, goofi_core::control::number_of);
        self.send(Msg::Configure(g.machines().clone(), (g.timeline().clone(), bpm), g.evaluator(), node_reads(g)));
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

/// Every `nd()` term of the stepped expressions, resolved as a param's binding resolves it.
fn node_reads(g: &Graph) -> NodeReads {
    let mut out = NodeReads::default();
    for target in g.stepped_refs() {
        match target {
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
        // The recordings to end at the next pass, each with who waits for what it took.
        let mut ended: Vec<(String, Sender<Take>)> = Vec::new();
        loop {
            // Park toward the next instant owed; a ring — a frame read, or the inbox — is the other
            // reason to wake.
            // With nothing that steps, nothing is owed, a restart of the position included.
            let owed = time.until(machines.next_deadline().unwrap_or(f64::INFINITY)).filter(|_| !machines.is_empty());
            let wait = owed.map_or(IDLE, |left| left.min(IDLE));
            let mut woken = door.wait_until(Instant::now() + wait).has(MACHINES_FRAME_ID);
            loop {
                match rx.try_recv() {
                    Ok(msg) => woken |= take(&mut machines, &mut reads, &mut streams, &mut ended, &state.hub, msg),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
            if state.stopping.stopped() {
                return;
            }
            if machines.is_empty() {
                // No track is left to have recorded: who waits hears that as the answer hung up.
                ended.clear();
                continue;
            }
            // The position went back — a jump, a loop's restart — and that is owed too.
            let now = time.position();
            if !woken && !machines.behind(now) && machines.next_deadline().is_none_or(|at| at > now) {
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
            // A recording ends with this instant in it; one that never ran hangs up on who waits.
            for (track, answer) in ended.drain(..) {
                if let Some(taken) = machines.recorded(&track, now, &read) {
                    let _ = answer.send(taken);
                }
            }
            let writes = machines.advance(now, &read);
            {
                let mut store = store.lock();
                for (name, value) in writes {
                    store.drive(&name, value);
                }
            }
            for (port, pitch, velocity) in machines.strike(time.playing()) {
                state.machines.ports.strike(&port, pitch, velocity);
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
fn take(machines: &mut Machines, reads: &mut NodeReads, streams: &mut Streams, ended: &mut Vec<(String, Sender<Take>)>, hub: &Hub, msg: Msg) -> bool {
    match msg {
        Msg::Configure(config, timed, evaluator, node_reads) => {
            machines.configure(config, timed, evaluator);
            streams.follow(hub, &node_reads);
            *reads = node_reads;
        }
        Msg::Fire { at, machine, playhead, transition } => machines.fire(at, &machine, &playhead, &transition),
        Msg::Jump { at, machine, playhead, state } => machines.jump(at, &machine, &playhead, &state),
        Msg::Reset { at, machine } => machines.reset(at, machine.as_deref()),
        // A pause and a resume are a pass too: the notes that sound stop, and start again.
        Msg::Changed(name) => return name == "clock.playing" || machines.keys().contains(&name),
        Msg::Jumped => machines.restart(),
        Msg::Section => return false,
        Msg::Record(track) => machines.record(&track),
        Msg::Recorded(track, answer) => ended.push((track, answer)),
    }
    true
}
