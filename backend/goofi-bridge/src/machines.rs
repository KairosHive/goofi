//! The machines' own thread, `goofi-machines`: it steps every state machine on the viewer-cap
//! pace, reading the variables their expressions name and writing each playhead's group through
//! the store, as the follower does — no graph lock, no undo entry, no dirty mark. The graph lock
//! is for config edits, which hand it a replaced model.

use std::collections::HashSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use goofi_core::indexmap::IndexMap;
use goofi_graph::machine::{Machine, Machines};
use goofi_graph::Graph;

use crate::{reducer, AppState};

pub enum Msg {
    Configure(IndexMap<String, Machine>, Option<Arc<dyn goofi_node::ExprEvaluator>>),
    Fire { machine: String, playhead: String, transition: String },
    Jump { machine: String, playhead: String, state: String },
    Reset { machine: Option<String> },
}

/// The manager's hand on the thread.
pub struct Driver {
    tx: Sender<Msg>,
}

impl Driver {
    pub fn new() -> (Driver, Receiver<Msg>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (Driver { tx }, rx)
    }

    /// Hand the thread the settled model.
    pub fn configure(&self, g: &Graph) {
        let _ = self.tx.send(Msg::Configure(g.machines().clone(), g.evaluator()));
    }

    pub fn send(&self, msg: Msg) {
        let _ = self.tx.send(msg);
    }
}

/// How long the thread sleeps between looks at its inbox while no machine runs.
const IDLE: Duration = Duration::from_millis(500);

pub fn spawn(state: AppState, rx: Receiver<Msg>) {
    let owner = state.clone();
    owner.scope.spawn("goofi-machines", move || {
        let (store, time) = {
            let g = state.graph.lock();
            (g.variable_store(), g.time())
        };
        let mut machines = Machines::default();
        let mut pace = reducer::Pace::new();
        let mut logged: HashSet<String> = HashSet::new();
        loop {
            let interval = state.reducers.cap_interval();
            let due = if machines.is_empty() { Instant::now() + IDLE } else { pace.due(interval, Instant::now()) };
            loop {
                match rx.recv_timeout(due.saturating_duration_since(Instant::now())) {
                    Ok(Msg::Configure(config, evaluator)) => machines.configure(config, evaluator),
                    Ok(Msg::Fire { machine, playhead, transition }) => machines.fire(&machine, &playhead, &transition),
                    Ok(Msg::Jump { machine, playhead, state }) => machines.jump(&machine, &playhead, &state),
                    Ok(Msg::Reset { machine }) => machines.reset(machine.as_deref()),
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            if state.stopping.stopped() {
                return;
            }
            if machines.is_empty() {
                continue;
            }
            pace.take(interval, Instant::now());
            // The frames the expressions read, taken in one look; the step runs off every lock.
            let frames: IndexMap<String, goofi_core::Data> = {
                let store = store.lock();
                machines.keys().into_iter().filter_map(|k| Some((k.clone(), store.get(&k)?.clone()))).collect()
            };
            let writes = machines.advance(time.now(), &|key| frames.get(key).cloned());
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
