//! The one way goofi starts a thread that outlives its caller: named, listed while it runs, and
//! joinable within a deadline so a hung thread cannot hold a shutdown hostage.

use std::io;
use std::sync::Arc;
use crate::sync::Latch;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::scope::{self, Kind};

/// A thread being described: its name, and the stack it gets.
pub struct Builder {
    name: String,
    stack: Option<usize>,
}

/// Describe a thread named `name`.
pub fn thread(name: impl Into<String>) -> Builder {
    Builder { name: name.into(), stack: None }
}

/// Start `f` on a thread named `name`. The handle may be dropped: the thread runs on, listed
/// until it ends.
pub fn spawn<T: Send + 'static>(name: impl Into<String>, f: impl FnOnce() -> T + Send + 'static) -> io::Result<Worker<T>> {
    thread(name).spawn(f)
}

impl Builder {
    pub fn stack_size(mut self, bytes: usize) -> Self {
        self.stack = Some(bytes);
        self
    }

    pub fn spawn<T: Send + 'static>(self, f: impl FnOnce() -> T + Send + 'static) -> io::Result<Worker<T>> {
        let lease = scope::lease(Kind::Worker, self.name.clone());
        let done = Arc::new(Latch::default());
        let finished = done.clone();
        let mut builder = std::thread::Builder::new().name(self.name.clone());
        if let Some(stack) = self.stack {
            builder = builder.stack_size(stack);
        }
        let handle = builder.spawn(move || {
            // Held by the thread itself: the entry is there exactly while the thread runs.
            let _lease = lease;
            let _ending = Ending(finished);
            f()
        })?;
        Ok(Worker { handle, done })
    }
}

/// Marks the thread finished on every exit, a panic included.
struct Ending(Arc<Latch>);

impl Drop for Ending {
    fn drop(&mut self) {
        self.0.open();
    }
}

/// A running thread. Dropping the handle detaches it; the thread stays listed until it ends.
pub struct Worker<T = ()> {
    handle: JoinHandle<T>,
    done: Arc<Latch>,
}

/// A watch on a thread's end that any number of holders can wait on, the handle kept elsewhere.
#[derive(Clone)]
pub struct Done(Arc<Latch>);

impl Done {
    /// Wait up to `within` for the thread to end; whether it did.
    pub fn wait_within(&self, within: Duration) -> bool {
        self.0.wait_until(Instant::now() + within)
    }
}

impl<T> Worker<T> {
    /// The watch on this thread's end.
    pub fn done(&self) -> Done {
        Done(self.done.clone())
    }

    /// Wait for the thread to end, however long that takes.
    pub fn join(self) -> std::thread::Result<T> {
        self.handle.join()
    }

    /// Whether the thread has ended.
    pub fn is_done(&self) -> bool {
        self.done.is_open()
    }

    /// Wait up to `within` for the thread to end. `None` is the deadline: the thread runs on,
    /// detached, and stays listed until it ends.
    pub fn join_within(self, within: Duration) -> Option<std::thread::Result<T>> {
        self.done().wait_within(within).then(|| self.handle.join())
    }
}
