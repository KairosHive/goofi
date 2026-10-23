//! A component made ready off the thread that will use it, and swapped in whole. The rule for
//! every engine and node: compilation and configuration — a shader's pipeline, a plugin's
//! instantiation, a device's opening — run on a thread of their own, the system keeps running
//! on what it has, and the owner takes the result at a safe point of its own choosing. Nothing
//! waits on a build; nothing is half-built where a tick can see it.
//!
//! Use [`prepare`] to start the work and keep the [`Ready`] it answers. The owner asks [`Ready::peek`]
//! (a `Clone` result it reads in place, a pipeline) or [`Ready::take`] (a result it moves in, a
//! node) at its next tick or settle; [`Ready::on_ready`] rings whatever that owner parks on, so
//! the swap costs no poll.

use std::sync::Arc;

use crate::sync::Mutex;

/// The outcome of one preparation, filled once; what the owner reads at its safe point.
pub struct Ready<T>(Arc<Cell<T>>);

impl<T> Clone for Ready<T> {
    fn clone(&self) -> Ready<T> {
        Ready(self.0.clone())
    }
}

struct Cell<T> {
    /// `None` until the work is done; a taken result leaves `Some(Taken)` behind.
    outcome: Mutex<Outcome<T>>,
    /// What to wake once the outcome is in; each once, in the order they asked.
    wake: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
}

enum Outcome<T> {
    Pending,
    Done(Result<T, String>),
    Taken,
}

impl<T> Ready<T> {
    fn new() -> Ready<T> {
        Ready(Arc::new(Cell { outcome: Mutex::new(Outcome::Pending), wake: Mutex::new(Vec::new()) }))
    }

    /// A result that is in from the start: a build refused before it began.
    pub fn done(outcome: Result<T, String>) -> Ready<T> {
        let ready = Ready::new();
        ready.set(outcome);
        ready
    }

    fn set(&self, outcome: Result<T, String>) {
        *self.0.outcome.lock() = Outcome::Done(outcome);
        for wake in std::mem::take(&mut *self.0.wake.lock()) {
            wake();
        }
    }

    /// Whether the work is done, whichever way.
    pub fn is_ready(&self) -> bool {
        !matches!(*self.0.outcome.lock(), Outcome::Pending)
    }

    /// Ring `wake` once the outcome is in — at once if it already is.
    pub fn on_ready(&self, wake: impl FnOnce() + Send + 'static) {
        let mut wakes = self.0.wake.lock();
        if self.is_ready() {
            drop(wakes);
            wake();
        } else {
            wakes.push(Box::new(wake));
        }
    }

    /// The outcome, moved out once: `None` while pending, and again once taken.
    pub fn take(&self) -> Option<Result<T, String>> {
        let mut outcome = self.0.outcome.lock();
        match std::mem::replace(&mut *outcome, Outcome::Taken) {
            Outcome::Done(result) => Some(result),
            Outcome::Pending => {
                *outcome = Outcome::Pending;
                None
            }
            Outcome::Taken => None,
        }
    }
}

impl<T: Clone> Ready<T> {
    /// The outcome, read in place: `None` while pending.
    pub fn peek(&self) -> Option<Result<T, String>> {
        match &*self.0.outcome.lock() {
            Outcome::Done(result) => Some(result.clone()),
            Outcome::Pending | Outcome::Taken => None,
        }
    }

    /// The error, if the work failed.
    pub fn error(&self) -> Option<String> {
        match &*self.0.outcome.lock() {
            Outcome::Done(Err(why)) => Some(why.clone()),
            _ => None,
        }
    }
}

/// Run `work` on a thread named `name` and answer the [`Ready`] it fills. A thread that could not
/// start, and a work that panicked, are works that failed, which the owner reads like any other
/// refusal: nothing is left pending for ever.
pub fn prepare<T: Send + 'static>(name: impl Into<String>, work: impl FnOnce() -> Result<T, String> + Send + 'static) -> Ready<T> {
    let ready = Ready::new();
    let filled = ready.clone();
    let name = name.into();
    let spawned = crate::worker::thread(name.clone()).spawn(move || {
        let outcome = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
            Ok(outcome) => outcome,
            Err(payload) => Err(format!("panicked: {}", panic_message(&payload))),
        };
        filled.set(outcome);
    });
    if let Err(e) = spawned {
        ready.set(Err(format!("{name}: no thread to prepare on: {e}")));
    }
    ready
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a payload that is not text".to_string())
}
