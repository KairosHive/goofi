//! The patch's time: one origin, read as seconds since the patch began.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use std::time::{Instant, SystemTime};

/// The process base an origin is stored against, so the origin itself is one atomic — which is
/// what lets a render or callback thread read the time with no lock.
static BASE: LazyLock<Instant> = LazyLock::new(Instant::now);

fn since_base() -> u64 {
    BASE.elapsed().as_nanos() as u64
}

/// The patch's time. Every engine, every node and every stamp reads THIS object; nothing copies
/// what it says. A load restarts it, because a patch loaded an hour in begins at zero.
#[derive(Debug)]
pub struct Time {
    origin: AtomicU64,
    wall: std::sync::Mutex<SystemTime>,
}

impl Default for Time {
    fn default() -> Time {
        Time::new()
    }
}

impl Time {
    pub fn new() -> Time {
        Time { origin: AtomicU64::new(since_base()), wall: std::sync::Mutex::new(SystemTime::now()) }
    }

    /// Seconds since the patch began.
    pub fn now(&self) -> f64 {
        since_base().saturating_sub(self.origin.load(Ordering::Relaxed)) as f64 / 1e9
    }

    /// The wall time the patch began at — the one UTC a recording's manifest states, so nothing
    /// downstream mints a second origin.
    pub fn wall(&self) -> SystemTime {
        *self.wall.lock().expect("a poisoned time is a panicked writer")
    }

    /// Begin again from now.
    pub fn restart(&self) {
        self.origin.store(since_base(), Ordering::Relaxed);
        *self.wall.lock().expect("a poisoned time is a panicked writer") = SystemTime::now();
    }
}
