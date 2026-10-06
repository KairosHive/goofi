//! The machines' own thread, `goofi-machines`: it wakes for a reason alone — a landing, a dwell's
//! due, a stamped request, a replaced model, or a write to a variable a machine reads — settles
//! every instant owed and writes each playhead's group through the store, as the follower does:
//! no graph lock, no undo entry, no dirty mark. The graph lock is for config edits, which hand it
//! a replaced model.

use std::collections::HashSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

use goofi_core::indexmap::IndexMap;
use goofi_graph::machine::{Machine, Machines};
use goofi_graph::Graph;

use crate::AppState;

pub enum Msg {
    Configure(IndexMap<String, Machine>, Option<Arc<dyn goofi_node::ExprEvaluator>>),
    /// An effect, stamped with the patch second of the op that asked for it.
    Fire { at: f64, machine: String, playhead: String, transition: String },
    Jump { at: f64, machine: String, playhead: String, state: String },
    Reset { at: f64, machine: Option<String> },
    /// A variable was written; one a machine reads is a reason to look again.
    Changed(String),
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

    /// The store's word that a variable moved, for the thread's inbox.
    pub fn watch(&self) -> Arc<dyn Fn(&str) + Send + Sync> {
        let tx = self.tx.clone();
        Arc::new(move |name: &str| {
            let _ = tx.send(Msg::Changed(name.to_string()));
        })
    }
}

/// How long the thread sleeps between looks at its inbox while nothing is owed.
const IDLE: Duration = Duration::from_millis(500);

pub fn spawn(state: AppState, rx: Receiver<Msg>) {
    let owner = state.clone();
    owner.scope.spawn("goofi-machines", move || {
        let (store, time) = {
            let g = state.graph.lock();
            (g.variable_store(), g.time())
        };
        let mut machines = Machines::default();
        let mut logged: HashSet<String> = HashSet::new();
        loop {
            // Sleep toward the next instant owed; a message is the other reason to wake.
            let wait = match machines.next_deadline() {
                Some(at) => Duration::from_secs_f64((at - time.now()).max(0.0)).min(IDLE),
                None => IDLE,
            };
            let mut woken = false;
            match rx.recv_timeout(wait) {
                Ok(msg) => woken |= take(&mut machines, msg),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            while let Ok(msg) = rx.try_recv() {
                woken |= take(&mut machines, msg);
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
            let frames: IndexMap<String, goofi_core::Data> = {
                let store = store.lock();
                machines.keys().into_iter().filter_map(|k| Some((k.clone(), store.get(&k)?.clone()))).collect()
            };
            let writes = machines.advance(now, &|key| frames.get(key).cloned());
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
fn take(machines: &mut Machines, msg: Msg) -> bool {
    match msg {
        Msg::Configure(config, evaluator) => machines.configure(config, evaluator),
        Msg::Fire { at, machine, playhead, transition } => machines.fire(at, &machine, &playhead, &transition),
        Msg::Jump { at, machine, playhead, state } => machines.jump(at, &machine, &playhead, &state),
        Msg::Reset { at, machine } => machines.reset(at, machine.as_deref()),
        Msg::Changed(name) => return machines.keys().contains(&name),
    }
    true
}
