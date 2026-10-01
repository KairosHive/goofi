//! The services and ports: one shape table every end opens through, bells, publishing, and the halt a teardown parks on.

use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iceoryx2::prelude::*;

use goofi_node::EventId;

use crate::names::RECORD_EVENT_ID;

use crate::{ByteService, BytePublisher, ByteSubscriber, EventService, IoxNode, Svc};

/// `EventId(0)` is a control message; `1..=64` an input slot, the slots past 63 sharing 64;
/// `65..=128` an `nd()` channel. 255 is the ceiling those three ranges are budgeted against.
const EVENT_ID_MAX: usize = 255;
/// One notifier per producer feeding this node, plus the graph. The default 16 busts on 20 wires.
const MAX_NOTIFIERS: usize = 256;
/// Fan-out plus the `/data` reducer. The default 8 busts on a 9-consumer slot.
const MAX_SUBSCRIBERS: usize = 256;
/// How many iceoryx2 NODES may open one service: one graph node is one iceoryx2 node, so this is
/// a per-peer bound, and it binds below both ceilings above.
const MAX_NODES: usize = 256;
/// The pool a data publisher starts with; `PowerOfTwo` grows it for a larger frame.
pub const INITIAL_SLICE: usize = 64 * 1024;
/// The largest frame a SIGNAL recording service takes: 1 MiB clears a 64-channel, 2500-sample
/// frame with room, and a frame over it is refused rather than grown into.
pub const RECORD_SLICE: usize = 1024 * 1024;
/// The largest frame an AUDIO recording service takes: one block of the widest output, with room.
pub const AUDIO_RECORD_SLICE: usize = 64 * 1024;
/// What one armed slot's segment costs, exactly and for any frame size — the publisher allocates
/// [`RecordShape::buffer`] slices of [`RecordShape::slice`] and never grows.
pub const RECORD_BUDGET: usize = 64 * 1024 * 1024;

/// How one armed slot's segment is cut: one budget buys 64 signal frames or 1024 audio blocks. A
/// reader slower than that depth loses the OLDEST frame, which the recorder counts by index.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecordShape {
    pub slice: usize,
    pub buffer: usize,
}

/// The shape an engine's armed slots publish with and the recorder subscribes with. Both ends
/// derive it from the engine name, so the service they meet on has only one description.
pub fn record_shape(engine: &str) -> RecordShape {
    let slice = match engine {
        "audio" => AUDIO_RECORD_SLICE,
        _ => RECORD_SLICE,
    };
    RecordShape { slice, buffer: RECORD_BUDGET / slice }
}

/// A notifier onto one node's door; the ringer knows nothing else about the node it rings.
pub struct Doorbell(iceoryx2::port::notifier::Notifier<Svc>);

impl Doorbell {
    /// Open a door by name, on the ringer's OWN iceoryx2 node — one bell per producing NODE, since
    /// each node counts against `max_nodes`. `open_or_create`: the service is the rendezvous.
    pub fn open(node: &IoxNode, service: &str) -> Result<Doorbell, String> {
        let door = event_service(node, service)?;
        Ok(Doorbell(door.notifier_builder().create().map_err(|e| format!("notifier `{service}`: {e}"))?))
    }

    /// Ring it. A failed ring costs a wake, never a message: the payload is already in a queue the
    /// node drains.
    pub fn ring(&self, id: EventId) -> Result<(), String> {
        self.0
            .notify_with_custom_event_id(iceoryx2::prelude::EventId::new(id as usize))
            .map(|_| ())
            .map_err(|e| format!("notify: {e}"))
    }
}

/// The byte-stream services, one table: whichever side opens a service first fixes its shape for
/// the other, so every end is built through [`stream_service`] and none can disagree on a limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceKind {
    /// A data wire: one producer; no history, because a link never replays; one deep, latest wins.
    Data,
    /// A recorder's own service on an output slot: one reader, and a buffer deep enough that a
    /// journal commit costs no frames. Depth is per service, so this is never the shared data one.
    Record(RecordShape),
    /// The request or response service of one spawned node child.
    Exchange,
}

impl ServiceKind {
    /// How deep each subscriber's buffer is, and how many subscribers the service takes.
    fn shape(self) -> (usize, usize) {
        match self {
            ServiceKind::Data => (1, MAX_SUBSCRIBERS),
            ServiceKind::Record(shape) => (shape.buffer, 1),
            ServiceKind::Exchange => (2, 16),
        }
    }
}

/// A byte-stream service of `kind`, `open_or_create`: whichever side settles first waits for the
/// other. One publisher always, because a slot, a mailbox and a request each have one writer.
pub fn stream_service(node: &IoxNode, name: &str, kind: ServiceKind) -> Result<ByteService, String> {
    let (buffer, subscribers) = kind.shape();
    node.service_builder(&parse_name(name)?)
        .publish_subscribe::<[u8]>()
        .max_nodes(MAX_NODES)
        .enable_safe_overflow(true)
        .history_size(0)
        .subscriber_max_buffer_size(buffer)
        .max_publishers(1)
        .max_subscribers(subscribers)
        .open_or_create()
        .map_err(|e| format!("{kind:?} service `{name}`: {e}"))
}

/// The event service every door is, ONE configuration because the first opener fixes it. Two
/// listeners, so a reducer reborn on a slot can park before the one it replaces has left.
pub fn event_service(node: &IoxNode, name: &str) -> Result<EventService, String> {
    node.service_builder(&parse_name(name)?)
        .event()
        .max_nodes(MAX_NODES)
        .event_id_max_value(EVENT_ID_MAX)
        .max_notifiers(MAX_NOTIFIERS)
        .max_listeners(2)
        .open_or_create()
        .map_err(|e| format!("event service `{name}`: {e}"))
}

/// How many subscribers a data service has right now — whether anyone drinks from it.
pub fn subscribers(service: &ByteService) -> usize {
    service.dynamic_config().number_of_subscribers()
}

/// Open a subscriber on an output slot's data service by name — a `/data` consumer's end of a wire.
pub fn open_output_subscriber(node: &IoxNode, service: &str) -> Result<ByteSubscriber, String> {
    subscriber(&stream_service(node, service, ServiceKind::Data)?, service)
}

/// A producer's end of one armed slot's recording service: a STATIC segment of [`RECORD_BUDGET`].
/// Opened by the ARMING, released by the READER: a disarm only [retires](RecordPort::retire) it.
pub struct RecordPort {
    service: ByteService,
    publisher: BytePublisher,
    /// The node's ONE bell on the recorder's door, shared by every port it arms: a door opened
    /// twice on one node is a second service state iceoryx2 can drop out from under the first.
    bell: std::sync::Arc<Doorbell>,
    retired: bool,
}

impl RecordPort {
    pub fn open(node: &IoxNode, service: &str, bell: &std::sync::Arc<Doorbell>, what: &str, shape: RecordShape) -> Result<RecordPort, String> {
        let service = stream_service(node, service, ServiceKind::Record(shape))?;
        let publisher = service
            .publisher_builder()
            .initial_max_slice_len(shape.slice)
            .allocation_strategy(AllocationStrategy::Static)
            .create()
            .map_err(|e| format!("record publisher `{what}`: {e}"))?;
        Ok(RecordPort { service, publisher, bell: bell.clone(), retired: false })
    }

    /// One frame, written by `fill` into a loan of `len`, and the recorder's door rung. `false` is
    /// a refused loan, which the next frame's own number witnesses. A retired port sends nothing.
    pub fn send(&self, len: usize, fill: impl FnOnce(&mut [MaybeUninit<u8>])) -> bool {
        !self.retired && publish_with(&self.publisher, len, fill, std::iter::once((&*self.bell, RECORD_EVENT_ID))).is_ok()
    }

    /// Disarmed: stop publishing, but stay open while the recorder still reads.
    pub fn retire(&mut self) {
        self.retired = true;
    }

    pub fn armed(&mut self) {
        self.retired = false;
    }

    pub fn retired(&self) -> bool {
        self.retired
    }

    /// Retired, and nobody drinks from it any more — the one state in which dropping it is safe.
    pub fn spent(&self) -> bool {
        self.retired && subscribers(&self.service) == 0
    }
}

/// Open the recorder's end of an armed output slot's recording service.
pub fn open_record_subscriber(node: &IoxNode, service: &str, shape: RecordShape) -> Result<ByteSubscriber, String> {
    subscriber(&stream_service(node, service, ServiceKind::Record(shape))?, service)
}

pub fn subscriber(service: &ByteService, what: &str) -> Result<ByteSubscriber, String> {
    service.subscriber_builder().create().map_err(|e| format!("subscriber `{what}`: {e}"))
}

/// A publisher that can grow past its initial pool: a GOOF frame is variable-size, and `Static`
/// would refuse the first one larger than `initial` instead of reallocating.
pub fn publisher(service: &ByteService, what: &str, initial: usize) -> Result<BytePublisher, String> {
    service
        .publisher_builder()
        .initial_max_slice_len(initial)
        .allocation_strategy(AllocationStrategy::PowerOfTwo)
        .create()
        .map_err(|e| format!("publisher `{what}`: {e}"))
}

fn parse_name(name: &str) -> Result<iceoryx2::service::service_name::ServiceName, String> {
    name.try_into().map_err(|e| format!("bad service name `{name}`: {e:?}"))
}

/// Send one frame, then ring every bell: a consumer woken first drains nothing and parks. `Err`
/// is a failed loan or send, which is a caller's to count.
pub fn publish<'a>(publisher: &BytePublisher, bytes: &[u8], bells: impl IntoIterator<Item = (&'a Doorbell, EventId)>) -> Result<(), String> {
    publish_with(publisher, bytes.len(), |loan| write_parts(loan, [bytes]), bells)
}

/// [`publish`] with the frame written by `fill` straight into a loan of `len` bytes, so a frame
/// is encoded once, into shared memory. `fill` must write every byte.
pub fn publish_with<'a>(
    publisher: &BytePublisher,
    len: usize,
    fill: impl FnOnce(&mut [MaybeUninit<u8>]),
    bells: impl IntoIterator<Item = (&'a Doorbell, EventId)>,
) -> Result<(), String> {
    let mut sample = publisher.loan_slice_uninit(len).map_err(|e| format!("loan of {len} bytes: {e}"))?;
    fill(sample.payload_mut());
    // SAFETY: `fill`'s contract is that the whole loan was written.
    unsafe { sample.assume_init() }.send().map_err(|e| format!("send: {e}"))?;
    for (bell, id) in bells {
        let _ = bell.ring(id);
    }
    Ok(())
}

/// Copy `parts`, one after the other, over a loan they fill exactly.
pub fn write_parts<'p>(loan: &mut [MaybeUninit<u8>], parts: impl IntoIterator<Item = &'p [u8]>) {
    let mut at = 0;
    for part in parts {
        let end = at + part.len();
        // SAFETY: `MaybeUninit<u8>` and `u8` share a layout, and the range is bounds-checked.
        let dst = &mut loan[at..end];
        unsafe { std::ptr::copy_nonoverlapping(part.as_ptr(), dst.as_mut_ptr() as *mut u8, part.len()) };
        at = end;
    }
    assert_eq!(at, loan.len(), "a loan is filled exactly");
}

/// The survivor `keep` names, taken out of what a reconcile held; what is left is what the new
/// set does not name, and dropping it IS the unsubscribe.
pub fn take_where<T>(held: &mut Vec<T>, keep: impl Fn(&T) -> bool) -> Option<T> {
    held.iter().position(keep).map(|i| held.remove(i))
}

/// A CEILING on a teardown, not a join: a wedged node must not be able to wedge the exit. It is
/// the transport's own number because what the wait is FOR is the release of shared memory.
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

/// The two flags a node's thread is born holding: told to stop, and released once every port it
/// owned is dropped, which frees the shared memory a teardown waits for.
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
