//! The one way goofi starts a thread that outlives its caller: named, listed while it runs, and
//! joinable within a deadline so a hung thread cannot hold a shutdown hostage.

use std::io;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::registry::{self, Kind};

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
        let lease = registry::lease(Kind::Worker, self.name.clone());
        let done = Arc::new((Mutex::new(false), Condvar::new()));
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
        Ok(Worker { handle: Some(handle), done })
    }
}

/// Marks the thread finished on every exit, a panic included.
struct Ending(Arc<(Mutex<bool>, Condvar)>);

impl Drop for Ending {
    fn drop(&mut self) {
        let (flag, wake) = &*self.0;
        *flag.lock().unwrap_or_else(|e| e.into_inner()) = true;
        wake.notify_all();
    }
}

/// A running thread. Dropping the handle detaches it; the thread stays listed until it ends.
pub struct Worker<T = ()> {
    handle: Option<JoinHandle<T>>,
    done: Arc<(Mutex<bool>, Condvar)>,
}

impl<T> Worker<T> {
    /// Wait for the thread to end, however long that takes.
    pub fn join(mut self) -> std::thread::Result<T> {
        self.handle.take().expect("joined once").join()
    }

    /// Wait up to `within` for the thread to end. `None` is the deadline: the thread runs on,
    /// detached, and stays listed until it ends.
    pub fn join_within(mut self, within: Duration) -> Option<std::thread::Result<T>> {
        let (flag, wake) = &*self.done;
        let deadline = Instant::now() + within;
        let mut finished = flag.lock().unwrap_or_else(|e| e.into_inner());
        while !*finished {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            finished = wake.wait_timeout(finished, left).unwrap_or_else(|e| e.into_inner()).0;
        }
        drop(finished);
        Some(self.handle.take().expect("joined once").join())
    }
}
