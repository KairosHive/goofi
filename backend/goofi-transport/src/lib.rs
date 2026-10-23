//! goofi's transport: the in-process plane every engine, the recorder and the bridge share, and
//! the exchange a child is called over. Names are pure derivation from the graph, and the [`Hub`]
//! is where any end finds the other by that name, whichever settles first.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use goofi_supervisor::sync::Mutex;

pub mod door;
pub mod exchange;
mod link;
pub mod names;
pub mod queue;
mod shm;
pub mod slot;

pub use door::*;
pub use exchange::*;
pub use names::*;
pub use queue::*;
pub use slot::*;

/// A service name: a wire's identity, which is why a payload carries no source uid.
pub type ServiceName = String;

/// The id a door is rung with: `0` is a control message; `1..=64` an input slot, the slots past
/// 63 sharing 64; `65..=128` an `nd()` channel; the rest a door's own.
pub use goofi_node::EventId;

/// The per-session registry: every slot, door and queue by name, made by whichever end asks
/// first and gone when the last end lets go.
pub struct Hub {
    session: String,
    slots: Mutex<HashMap<String, Weak<slot::Cell>>>,
    doors: Mutex<HashMap<String, Weak<Door>>>,
    queues: Mutex<HashMap<String, Weak<queue::Queue>>>,
    /// Exchanges opened so far: each names its own mailboxes.
    exchanges: AtomicU64,
}

impl Hub {
    pub fn new(session: &str) -> Hub {
        Hub {
            session: session.to_string(),
            slots: Mutex::new(HashMap::new()),
            doors: Mutex::new(HashMap::new()),
            queues: Mutex::new(HashMap::new()),
            exchanges: AtomicU64::new(0),
        }
    }

    /// The session every name of this hub belongs to.
    pub fn id(&self) -> &str {
        &self.session
    }

    /// The entry `name` holds in `map`, or a fresh one; entries whose every end left are swept
    /// on the way.
    fn entry<T>(map: &Mutex<HashMap<String, Weak<T>>>, name: &str, make: impl FnOnce() -> T) -> Arc<T> {
        let mut map = map.lock();
        if let Some(live) = map.get(name).and_then(Weak::upgrade) {
            return live;
        }
        map.retain(|_, weak| weak.strong_count() > 0);
        let made = Arc::new(make());
        map.insert(name.to_string(), Arc::downgrade(&made));
        made
    }

    /// The one publisher of a slot. `held` keeps the newest payload for a subscriber that comes
    /// late, which a variable wants and a node's output never does.
    pub fn publisher(&self, name: &str, held: bool) -> Publisher {
        let cell = Self::entry(&self.slots, name, slot::Cell::default);
        cell.held.store(held, Ordering::Relaxed);
        Publisher::new(cell)
    }

    /// A reader of a slot — a wire's consumer end, a viewer's feed, a probe.
    pub fn subscribe(&self, name: &str) -> Subscriber {
        Subscriber::new(Self::entry(&self.slots, name, slot::Cell::default))
    }

    /// How many readers a slot has right now: its demand, as its producer reads it.
    pub fn readers(&self, name: &str) -> usize {
        self.slots.lock().get(name).and_then(std::sync::Weak::upgrade).map_or(0, |cell| cell.readers())
    }

    /// The door `name` names: parked on by its owner, rung by anyone holding it.
    pub fn door(&self, name: &str) -> Arc<Door> {
        Self::entry(&self.doors, name, || Door::new(name))
    }

    /// A producer's end of a queue `depth` frames deep: an armed slot's, feeding the recorder.
    pub fn sender(&self, name: &str, depth: usize) -> Sender {
        Sender::new(Self::entry(&self.queues, name, || queue::Queue::new(depth)))
    }

    /// The reader's end of a queue `depth` frames deep: both ends name the depth, from the
    /// engine, so whichever comes first makes the same queue.
    pub fn receiver(&self, name: &str, depth: usize) -> Receiver {
        Receiver::new(Self::entry(&self.queues, name, || queue::Queue::new(depth)))
    }

    /// A fresh exchange number: what names its mailboxes, unique for the session.
    pub(crate) fn next_exchange(&self) -> u64 {
        self.exchanges.fetch_add(1, Ordering::Relaxed)
    }
}

/// A path for a file needed for a moment — a `.gfi` packed or uploaded — under the session's
/// ephemeral directory, so a crash's leftover is swept with the session.
pub fn scratch(session: &str, name: &str) -> Result<std::path::PathBuf, String> {
    let dir = goofi_supervisor::session::system_dir(session).join("scratch");
    let _ = std::fs::create_dir_all(&dir);
    Ok(dir.join(name))
}

/// The survivor `keep` names, taken out of what a reconcile held; what is left is what the new
/// set does not name, and dropping it IS the unsubscribe.
pub fn take_where<T>(held: &mut Vec<T>, keep: impl Fn(&T) -> bool) -> Option<T> {
    held.iter().position(keep).map(|i| held.remove(i))
}

/// A CEILING on a teardown, not a join: a wedged node must not be able to wedge the exit.
pub const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

/// Wait for every halt to release, up to [`SHUTDOWN_WAIT`]. Whether they all did.
pub fn wait_released<'a>(halts: impl Iterator<Item = &'a Halt>) -> bool {
    let deadline = Instant::now() + SHUTDOWN_WAIT;
    let mut all = true;
    for halt in halts {
        all &= halt.wait_released_until(deadline);
    }
    all
}

thread_local! {
    static WORN: std::cell::RefCell<Option<Arc<Halt>>> = const { std::cell::RefCell::new(None) };
}

/// The two flags a node's thread is born holding: told to stop, and released once its thread
/// has let go of everything it held.
#[derive(Default)]
pub struct Halt {
    stop: AtomicBool,
    released: goofi_supervisor::sync::Latch,
}

impl Halt {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
    pub fn release(&self) {
        self.released.open();
    }
    /// Wear this halt on the calling thread: a wait the thread makes on another process ends
    /// when the halt is raised.
    pub fn wear(self: &Arc<Halt>) {
        WORN.with(|w| *w.borrow_mut() = Some(self.clone()));
    }
    /// Whether the halt the calling thread wears, if any, was raised.
    pub fn worn_stopped() -> bool {
        WORN.with(|w| w.borrow().as_ref().is_some_and(|h| h.stopped()))
    }
    /// Park until released or `deadline`; whether it was released.
    pub fn wait_released_until(&self, deadline: Instant) -> bool {
        self.released.wait_until(deadline)
    }
}
