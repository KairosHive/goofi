//! The engine's one expression worker. Every computed binding of its nodes is evaluated here,
//! never on a node's own thread, and the node reads the last result that stands: latest wins,
//! nothing waits, and a stalled interpreter holds the previous value.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{Arc, Weak};

use goofi_core::Data;
use goofi_node::{BindingId, EvalCtx, ExprEvaluator, Local};
use goofi_supervisor::scope::{self, Kind, Leased};
use goofi_supervisor::sync::Mutex;
use goofi_transport::{Doorbell, Iox, IoxNode};

/// One evaluation: the handle, every variable as it stands, and the context the harness wants.
pub struct Job {
    pub id: BindingId,
    pub locals: Vec<(String, Local)>,
    pub t: f64,
    pub range: (f64, f64),
}

/// One binding's handover. The worker owns only a weak registration, so replacement retires it.
pub struct Cell {
    door: String,
    pending: Mutex<Option<Job>>,
    done: Mutex<Option<Result<Data, String>>>,
    owner: Mutex<Option<Weak<Work>>>,
}

impl Cell {
    pub fn new(door: String) -> Cell {
        Cell { door, pending: Mutex::new(None), done: Mutex::new(None), owner: Mutex::new(None) }
    }

    /// The result since the last read, if one landed.
    pub fn take(&self) -> Option<Result<Data, String>> {
        self.done.lock().take()
    }
}

impl Drop for Cell {
    fn drop(&mut self) {
        let pointer = self as *const Cell;
        if let Some(work) = self.owner.lock().as_ref().and_then(Weak::upgrade) {
            let mut state = work.state.lock();
            state.cells.retain(|entry| !std::ptr::eq(entry.cell.as_ptr(), pointer));
            state.prune();
            let _ = work.wake.try_send(());
        }
    }
}

pub type SharedEvaluator = Arc<Mutex<Option<Arc<dyn ExprEvaluator>>>>;

struct Registration {
    cell: Weak<Cell>,
    door: String,
}

#[derive(Default)]
struct WorkState {
    cells: Vec<Registration>,
    bells: HashMap<String, Arc<Leased<Doorbell>>>,
}

impl WorkState {
    fn prune(&mut self) {
        self.cells.retain(|entry| entry.cell.strong_count() > 0);
        self.bells.retain(|door, _| self.cells.iter().any(|entry| &entry.door == door));
    }
}

struct Work {
    state: Mutex<WorkState>,
    wake: SyncSender<()>,
    engine: String,
}

pub struct Worker {
    work: Arc<Work>,
}

impl Worker {
    /// Start the engine's worker; it ends with the last `Worker` that holds its worklist.
    pub fn start(engine: &str, iox: &Iox, evaluator: SharedEvaluator) -> Result<Worker, String> {
        let node = iox.node()?;
        let (wake, rx) = std::sync::mpsc::sync_channel(1);
        let work = Arc::new(Work { state: Mutex::new(WorkState::default()), wake, engine: engine.into() });
        let owner = Arc::downgrade(&work);
        goofi_transport::thread(format!("goofi-{engine}-expr"))
            .spawn(move || serve(rx, owner, evaluator, node))
            .map_err(|e| format!("could not start the expression worker: {e}"))?;
        Ok(Worker { work })
    }

    /// Replace the binding's one pending job and coalesce notification of the live worklist.
    pub fn submit(&self, cell: &Arc<Cell>, job: Job) {
        *cell.owner.lock() = Some(Arc::downgrade(&self.work));
        let mut state = self.work.state.lock();
        state.prune();
        if !state.cells.iter().any(|entry| std::ptr::eq(entry.cell.as_ptr(), Arc::as_ptr(cell))) {
            state.cells.push(Registration { cell: Arc::downgrade(cell), door: cell.door.clone() });
        }
        *cell.pending.lock() = Some(job);
        let _ = self.work.wake.try_send(());
    }
}

fn serve(rx: Receiver<()>, owner: Weak<Work>, evaluator: SharedEvaluator, node: IoxNode) {
    goofi_supervisor::log::set_source(goofi_supervisor::log::Source::component("expr"));
    while rx.recv().is_ok() {
        let Some(work) = owner.upgrade() else { return };
        loop {
            let cells: Vec<_> = work.state.lock().cells.iter().map(|entry| entry.cell.clone()).collect();
            let next = cells.into_iter().find_map(|weak| {
                let cell = weak.upgrade()?;
                let job = cell.pending.lock().take()?;
                Some((weak, job))
            });
            let Some((cell, job)) = next else { break };
            // Put the selected binding last so another live binding gets its next turn.
            {
                let mut state = work.state.lock();
                if let Some(index) = state.cells.iter().position(|entry| Weak::ptr_eq(&entry.cell, &cell)) {
                    let entry = state.cells.remove(index);
                    state.cells.push(entry);
                }
            }
            let result = match evaluator.lock().clone() {
                Some(ev) => goofi_node::samples::eval_frame(ev.as_ref(), job.id, &EvalCtx { locals: &job.locals, t: job.t, range: job.range }).map_err(|e| e.0),
                None => Err("no expression evaluator available".to_string()),
            };
            // Evaluation holds no strong cell: a retired binding owns neither completion nor port.
            if let Some(cell) = cell.upgrade() {
                *cell.done.lock() = Some(result);
                ring(&work, &node, &cell.door);
            }
        }
    }
}

/// One notifier per live node door, released with its last binding even while evaluation stalls.
fn ring(work: &Work, node: &IoxNode, door: &str) {
    let held = work.state.lock().bells.get(door).cloned();
    let bell = match held {
        Some(bell) => bell,
        None => {
            let Ok(bell) = Doorbell::open(node, door) else { return };
            let bell = Arc::new(scope::leased(Kind::Port, format!("{} expression notification {door}", work.engine), bell));
            work.state.lock().bells.entry(door.into()).or_insert(bell).clone()
        }
    };
    if bell.ring(0).is_err() {
        work.state.lock().bells.remove(door);
    }
}
