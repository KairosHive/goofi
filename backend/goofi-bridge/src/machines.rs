//! The machine runtime owns logical event times. A wake asks it to catch up; it does not
//! move a deadline. Viewer demand only paces the samples published while a flight runs.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

use goofi_core::indexmap::IndexMap;
use goofi_core::time::{seconds, Tick, Time};
use goofi_graph::machine::{Machine, Machines};
use goofi_graph::Graph;
use goofi_supervisor::sync::Mutex;

use crate::AppState;

pub use goofi_graph::machine::Effect;

type EffectReply = Sender<Result<(), String>>;

enum Event {
    Inputs(Vec<(String, Option<goofi_core::Data>)>),
    Frame { key: String, service: String, frame: goofi_core::Data },
    Configure(IndexMap<String, Machine>, Option<Arc<dyn goofi_node::ExprEvaluator>>, Vec<(String, Option<goofi_core::Data>)>, IndexMap<String, u64>, Arc<goofi_graph::variable_projection::VariableProjection>, HashMap<String, String>),
    Effect(Effect, EffectReply),
}

impl goofi_core::variables::Watch for Driver {
    fn changed(&self, values: Vec<(String, Option<goofi_core::Data>)>) {
        let tx = self.tx.lock();
        let (epoch, at) = self.time.stamp();
        let _ = tx.send(Msg { epoch, at, event: Event::Inputs(values) });
    }
}

pub struct Msg {
    epoch: u64,
    at: Tick,
    event: Event,
}

/// The queue lock orders timestamp assignment and enqueue with the worker's clock cutoff.
pub struct Driver {
    tx: Mutex<Sender<Msg>>,
    time: Arc<Time>,
}

impl Driver {
    /// Deliver a raw leaf before any asynchronous alias evaluation.
    pub fn frame(&self, key: String, service: String, frame: goofi_core::Data) {
        let tx = self.tx.lock();
        let (current_epoch, arrival) = self.time.stamp();
        let epoch = goofi_core::samples::SampleSpan::of(&frame).map_or(current_epoch, |span| span.clock.patch_epoch);
        let at = goofi_core::samples::SampleSpan::of(&frame).map_or(arrival, |span|
            span.clock.at(span.first).saturating_sub(goofi_core::samples::CONTROL_DELAY));
        let _ = tx.send(Msg { epoch, at, event: Event::Frame { key, service, frame } });
    }
    pub fn new(time: Arc<Time>) -> (Driver, Receiver<Msg>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (Driver { tx: Mutex::new(tx), time }, rx)
    }

    pub fn configure(&self, g: &Graph) {
        let projection = g.variable_projection();
        let sources = projection.stream_inputs().into_iter().map(|(key, uid, _, slot)| (key, crate::output_service_of(g, uid, slot))).collect();
        let mut store = g.variables();
        let values = store.finish_publication(false);
        let producers = store.entries().filter_map(|(name, _)| Some((name.to_string(), store.generation(name)?))).collect();
        let tx = self.tx.lock();
        let (epoch, at) = self.time.stamp();
        let _ = tx.send(Msg { epoch, at, event: Event::Configure(g.machines().clone(), g.evaluator(), values, producers, projection, sources) });
    }

    /// Ask the runtime owner to accept an effect. The caller holds no graph lock.
    pub fn effect(&self, effect: Effect) -> Result<(), String> {
        let (answer, result) = std::sync::mpsc::channel();
        {
            let tx = self.tx.lock();
            let (epoch, at) = self.time.stamp();
            tx.send(Msg { epoch, at, event: Event::Effect(effect, answer) }).map_err(|_| "machine runtime stopped".to_string())?;
        }
        result.recv().map_err(|_| "machine runtime stopped".to_string())?
    }
}

const IDLE: Duration = Duration::from_millis(100);

pub fn spawn(state: AppState, rx: Receiver<Msg>) {
    let owner = state.clone();
    owner.scope.spawn("goofi-machines", move || {
        let time = {
            let g = state.graph.lock();
            g.time()
        };
        let mut machines = Machines::default();
        let mut logged: HashSet<String> = HashSet::new();
        let mut epoch = None;
        let mut frames = IndexMap::new();
        let mut sources = HashMap::new();
        let mut queued: VecDeque<Msg> = VecDeque::new();
        let mut awaiting: Vec<(EffectReply, Result<(), String>)> = Vec::new();
        loop {
            let now = time.now_ticks();
            let interval = if machines.is_empty() { IDLE } else { Duration::from_millis(1) };
            // Batch logical observation points; worker wakes do not define their times.
            let wait = machines.next_deadline().map_or(interval, |at| Duration::from_secs_f64(seconds(at.saturating_sub(now))).max(Duration::from_millis(1)).min(interval));
            let first = match rx.recv_timeout(wait) {
                Ok(msg) => Some(msg),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            if state.stopping.stopped() { return; }
            // No sender can stamp an event before this cutoff after the inbox is drained.
            let (batch, (current_epoch, cutoff)) = {
                let _queue = state.machines.tx.lock();
                (first.into_iter().chain(rx.try_iter()).collect::<Vec<_>>(), time.stamp())
            };
            if epoch != Some(current_epoch) {
                machines = Machines::default();
                epoch = Some(current_epoch);
                logged.clear();
                frames.clear();
                sources.clear();
                for msg in queued.drain(..) { if let Event::Effect(_, answer) = msg.event { let _ = answer.send(Err("the patch was replaced".into())); } }
                for (answer, _) in awaiting.drain(..) { let _ = answer.send(Err("the patch was replaced".into())); }
            }
            let clock = state.graph.lock().sample_clock().filter(|(clock, _)| clock.patch_epoch == current_epoch);
            let mut batch: Vec<_> = batch.into_iter().filter_map(|msg| {
                if msg.epoch == current_epoch { return Some(msg); }
                if let Event::Effect(_, answer) = msg.event {
                    let _ = answer.send(Err("the patch was replaced before this effect could run".into()));
                }
                None
            }).collect();
            // Ownership addresses come from the final settled source plan in this inbox.
            for msg in &batch {
                if let Event::Configure(_, _, _, _, _, expected) = &msg.event { sources = expected.clone(); }
            }
            batch.sort_by_key(|msg| msg.at);
            for msg in batch {
                match msg.event {
                    Event::Frame { key, service, frame } => {
                        if sources.get(&key) != Some(&service) { continue; }
                        let event = if goofi_core::samples::SampleSpan::of(&frame).is_some() {
                            Event::Frame { key, service, frame }
                        } else { Event::Inputs(vec![(key, Some(frame))]) };
                        queued.push_back(Msg { epoch: msg.epoch, at: msg.at, event });
                    }
                    event => queued.push_back(Msg { epoch: msg.epoch, at: msg.at, event }),
                }
            }
            queued.make_contiguous().sort_by_key(|msg| msg.at);
            loop {
                expire_inputs(&mut machines, cutoff);
                if let Some(at) = machines.pending_time() {
                    queued.retain(|msg| {
                        if msg.at <= at {
                            if let Event::Frame { key, service, frame } = &msg.event {
                                if sources.get(key) == Some(service) { land_frame(&mut machines, &mut frames, key.clone(), frame.clone()); }
                                return false;
                            }
                        }
                        true
                    });
                    let read = |key: &str| frames.get(key).cloned();
                    let writes = machines.advance(at, &read);
                    publish(&state, &machines, writes, clock);
                    if machines.pending_time().is_some() { break; }
                    for (answer, result) in awaiting.drain(..) { let _ = answer.send(result); }
                }
                let Some(at) = queued.front().map(|msg| msg.at).filter(|at| *at <= cutoff) else { break };
                let at = at.max(machines.logical_time().unwrap_or(at));
                let read = |key: &str| frames.get(key).cloned();
                if machines.logical_time().is_none_or(|previous| previous < at) {
                    let _ = machines.advance(at.saturating_sub(1), &read);
                }
                if machines.pending_time().is_some() { continue; }
                let mut change = None;
                let mut effects = VecDeque::new();
                let mut answers = VecDeque::new();
                while queued.front().is_some_and(|msg| msg.at <= at) {
                    let Some(msg) = queued.pop_front() else { break };
                    match msg.event {
                        Event::Inputs(values) => apply_inputs(&mut frames, values),
                        Event::Configure(config, evaluator, values, producers, projection, _) => {
                            apply_inputs(&mut frames, values);
                            change = Some((config, evaluator, producers, projection));
                        },
                        Event::Effect(effect, answer) => { effects.push_back(effect); answers.push_back(answer); }
                        Event::Frame { key, service, frame } => {
                            if sources.get(&key) == Some(&service) { land_frame(&mut machines, &mut frames, key, frame); }
                        },
                    }
                }
                if let Some((config, evaluator, producers, projection)) = change {
                    frames.retain(|key, _| producers.contains_key(key) || sources.contains_key(key));
                    machines.configure(at, config, evaluator, producers, projection);
                }
                let read = |key: &str| frames.get(key).cloned();
                let (accepted, pending) = machines.apply_effects(at, &mut effects, &read);
                for result in accepted { if let Some(answer) = answers.pop_front() { awaiting.push((answer, result)); } }
                if let Some(goofi_graph::variable_projection::ReadFault::Pending { source, at: requested }) = pending {
                    for (effect, answer) in effects.into_iter().zip(answers).rev() {
                        queued.push_front(Msg { epoch: current_epoch, at, event: Event::Effect(effect, answer) });
                    }
                    if cutoff >= requested.saturating_add(goofi_core::samples::CONTROL_DELAY) {
                        machines.input_error(source.clone(), Some(format!("input `{source}` missed coverage at {} within the control delay", seconds(requested))));
                        continue;
                    }
                    break;
                }
                let writes = machines.advance(at, &read);
                publish(&state, &machines, writes, clock);
                if machines.pending_time().is_some() { continue; }
                for (answer, result) in awaiting.drain(..) { let _ = answer.send(result); }
            }
            if machines.pending_time().is_none() && queued.front().is_none_or(|msg| msg.at > cutoff) {
                let read = |key: &str| frames.get(key).cloned();
                let writes = machines.advance(cutoff, &read);
                publish(&state, &machines, writes, clock);
            }
            let health = machines.health();
            let failures: HashSet<String> = health.iter().flat_map(|(name, _, health)| {
                health.expressions.iter().map(move |issue| format!("machine `{name}`, playhead {:?}, transition `{}`, {:?}, expression `{}`: {}", issue.playhead, issue.transition, issue.surface, issue.expression, issue.error))
                    .chain(health.playheads.iter().map(move |(ph, error)| format!("machine `{name}`, playhead `{ph}`: {error}")))
            }).collect();
            logged.retain(|why| failures.contains(why));
            for why in failures {
                if logged.insert(why.clone()) {
                    goofi_supervisor::log::record(goofi_supervisor::log::Source::component("machines"), goofi_supervisor::log::Level::Error, None, why);
                }
            }
            let mut graph = state.graph.lock();
            if !machines.matches_config(graph.machines()) || !machines.matches_projection(&graph) { continue; }
            if graph.report_machine_health(health) {
                let doc = state.doc.lock();
                let projection = graph.replica();
                drop(graph);
                crate::reconcile_and_broadcast(&state, doc, projection, Vec::new());
            }
        }
    });
}

fn publish(state: &AppState, machines: &Machines, writes: Vec<(String, goofi_core::Data)>, clock: Option<(goofi_core::samples::SampleClock, u64)>) {
    if machines.pending_time().is_some() { return; }
    let Some(at) = machines.logical_time() else { return };
    let mut samples: IndexMap<_, _> = clock.map(|(clock, next)| machines.sampled_writes(clock, next, at)).unwrap_or_default().into_iter().collect();
    let graph = state.graph.lock();
    if !machines.matches_config(graph.machines()) || !machines.matches_projection(&graph) { return; }
    let mut store = graph.variables();
    for (name, value) in writes { if let Some(owner) = machines.owner(&name) { store.drive(&name, owner, value, samples.shift_remove(&name)); } }
}

fn expire_inputs(machines: &mut Machines, now: Tick) {
    for (source, at) in machines.pending_inputs() {
        if now >= at.saturating_add(goofi_core::samples::CONTROL_DELAY) {
            machines.input_error(source.clone(), Some(format!("input `{source}` missed coverage at {} within the control delay", seconds(at))));
        }
    }
}

fn land_frame(machines: &mut Machines, frames: &mut IndexMap<String, goofi_core::Data>, key: String, frame: goofi_core::Data) {
    if frames.get(&key) == Some(&frame) { return; }
    let error = goofi_core::samples::SampleSpan::validate(&frame).err().or_else(|| {
        let span = goofi_core::samples::SampleSpan::of(&frame)?;
        let committed = machines.logical_time()?;
        let previous = frames.get(&key);
        for i in 0..span.length {
            let at = span.clock.at(span.first + i as u64).saturating_sub(goofi_core::samples::CONTROL_DELAY);
            if at >= committed { break; }
            let column = goofi_node::samples::at(&frame, at).ok().flatten();
            if previous.and_then(|old| goofi_node::samples::at(old, at).ok().flatten()) != column {
                return Some(format!("input `{key}` changed an instant already committed"));
            }
        }
        None
    });
    machines.input_error(key.clone(), error);
    frames.insert(key, frame);
}

/// Apply a committed input batch before any runtime decision.
fn apply_inputs(frames: &mut IndexMap<String, goofi_core::Data>, values: Vec<(String, Option<goofi_core::Data>)>) {
    for (name, value) in values {
        match value {
            Some(value) => { frames.insert(name, value); }
            None => { frames.shift_remove(&name); }
        }
    }
}
