//! The engine's one expression worker. Every computed binding of its nodes is evaluated here,
//! never on a node's own thread, and the node reads the last result that stands: latest wins,
//! nothing waits, and a stalled interpreter holds the previous value.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use goofi_core::Data;
use goofi_node::{BindingId, EvalCtx, ExprEvaluator, Local};
use goofi_supervisor::sync::Mutex;
use goofi_transport::{Doorbell, Iox, IoxNode};

/// One evaluation: the handle, every variable as it stands, and the context the harness wants.
pub struct Job {
    pub id: BindingId,
    pub locals: Vec<(String, Local)>,
    pub t: f64,
    pub range: (f64, f64),
}

/// One binding's handover: the job that stands to run and the result that stands to be read, each
/// replaced by the next. `door` is the node's, rung once a result is in.
pub struct Cell {
    door: String,
    pending: Mutex<Option<Job>>,
    done: Mutex<Option<Result<Data, String>>>,
}

impl Cell {
    pub fn new(door: String) -> Cell {
        Cell { door, pending: Mutex::new(None), done: Mutex::new(None) }
    }

    /// The result since the last read, if one landed.
    pub fn take(&self) -> Option<Result<Data, String>> {
        self.done.lock().take()
    }
}

pub type SharedEvaluator = Arc<Mutex<Option<Arc<dyn ExprEvaluator>>>>;

pub struct Worker {
    tx: Sender<Arc<Cell>>,
}

impl Worker {
    /// Start the engine's worker; it ends with the last `Worker` that holds its queue.
    pub fn start(engine: &str, iox: &Iox, evaluator: SharedEvaluator) -> Result<Worker, String> {
        let node = iox.node()?;
        let (tx, rx) = std::sync::mpsc::channel();
        goofi_transport::thread(format!("goofi-{engine}-expr"))
            .spawn(move || serve(rx, evaluator, node))
            .map_err(|e| format!("could not start the expression worker: {e}"))?;
        Ok(Worker { tx })
    }

    /// Hand a binding its next job; one already waiting is replaced.
    pub fn submit(&self, cell: &Arc<Cell>, job: Job) {
        *cell.pending.lock() = Some(job);
        let _ = self.tx.send(cell.clone());
    }
}

/// The worker's doors out, declared before the node they are opened on so they drop first.
struct Bells {
    open: HashMap<String, Doorbell>,
    node: IoxNode,
}

fn serve(rx: Receiver<Arc<Cell>>, evaluator: SharedEvaluator, node: IoxNode) {
    goofi_supervisor::log::set_source(goofi_supervisor::log::Source::component("expr"));
    let mut bells = Bells { open: HashMap::new(), node };
    for cell in rx {
        let Some(job) = cell.pending.lock().take() else { continue };
        let result = match evaluator.lock().clone() {
            Some(ev) => ev.eval(job.id, &EvalCtx { locals: &job.locals, t: job.t, range: job.range }).map_err(|e| e.0),
            None => Err("no expression evaluator available".to_string()),
        };
        *cell.done.lock() = Some(result);
        ring(&mut bells, &cell.door);
    }
}

/// Ring a node's door through the one bell this worker holds on it; a door that is gone is let go.
fn ring(bells: &mut Bells, door: &str) {
    if !bells.open.contains_key(door) {
        match Doorbell::open(&bells.node, door) {
            Ok(bell) => {
                bells.open.insert(door.to_string(), bell);
            }
            Err(_) => return,
        }
    }
    if bells.open[door].ring(0).is_err() {
        bells.open.remove(door);
    }
}
