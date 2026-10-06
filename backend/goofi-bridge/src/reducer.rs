//! Shared per-slot reducers: ONE reduction per watched `(node, slot)`, sized to the union of its
//! connections' specs and fanned out; it lives until its node leaves, and is woken, never clocked.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use goofi_supervisor::sync::Mutex;
use std::time::Duration;

use axum::body::Bytes;
use goofi_graph::{Graph, Uid};
use goofi_view::ViewSpec;
use tokio::sync::broadcast;

/// The physical stream a reducer serves: a node uid + one of its output slot names.
pub type SlotKey = (Uid, String);
/// A unique id per `/data` connection, so its spec contribution can be tracked + removed.
pub type ConnId = u64;
/// One variable's expression reading this slot, and the variable of that expression the frame
/// lands in.
#[derive(Clone, Debug, PartialEq)]
pub struct Tap {
    pub variable: String,
    pub var: String,
}

/// One computed variable as the follower holds it: the rewritten text, the evaluator's handle —
/// none for a bare source — and every variable it names.
pub struct Following {
    pub name: String,
    pub rewritten: String,
    pub id: Option<goofi_node::BindingId>,
    pub vars: Vec<(String, goofi_node::Var)>,
}

/// What reaches the follower: a tap's frame, or the whole set of variables it computes, from
/// settled state, with the evaluator the graph compiled them against.
pub enum Followed {
    Frame { variable: String, var: String, frame: goofi_core::Data },
    Desired(Vec<Following>, Option<Arc<dyn goofi_node::ExprEvaluator>>),
}

/// What one connection declares for a slot: the viewers' specs and the rate its display paints at.
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct Declared {
    #[serde(default)]
    pub specs: Vec<ViewSpec>,
    /// Frames a second; a connection that names none paints at the cap.
    #[serde(default)]
    pub fps: Option<f64>,
}

/// Flatten every connection's `ViewSpec`s into the single list the planner merges; an empty
/// list plans to the undeclared preview rather than to the full frame.
fn union_specs(by_conn: &HashMap<ConnId, Declared>) -> Vec<ViewSpec> {
    by_conn.values().flat_map(|d| d.specs.iter()).cloned().collect()
}

/// The viewer cap, `system.viewer_fps`: at least one frame a second.
fn cap_of(store: &goofi_core::variables::VariableStore) -> f64 {
    let fps = store.get("system.viewer_fps").map_or(0.0, goofi_core::control::number_of);
    if fps.is_finite() { fps.max(1.0) } else { 1.0 }
}

/// The gap between two serves of a slot: its fastest display's, never faster than the cap.
fn serve_interval(by_conn: &HashMap<ConnId, Declared>, cap: f64) -> Duration {
    let fps = by_conn.values().map(|d| d.fps.unwrap_or(cap)).fold(1.0, f64::max);
    Duration::from_secs_f64(1.0 / fps.min(cap))
}

/// A rate on ONE grid for the whole process: every serve lands on a tick of its interval, so
/// streams at one rate arrive together, a page paints them once, and work never stretches it.
pub(crate) struct Pace {
    tick: std::time::Instant,
    /// The grid `tick` is on; a tick armed on another grid is nobody's.
    interval: Duration,
    /// Whether something waits for `tick`: only then is a wake after it that tick's, late.
    armed: bool,
}

/// The first tick of `interval`'s grid at or after `at`.
fn tick(interval: Duration, at: std::time::Instant) -> std::time::Instant {
    static ORIGIN: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let origin = *ORIGIN.get_or_init(std::time::Instant::now);
    let step = interval.as_nanos().max(1);
    let n = at.saturating_duration_since(origin).as_nanos().div_ceil(step);
    origin + Duration::from_nanos((n * step) as u64)
}

impl Pace {
    pub(crate) fn new() -> Pace {
        Pace { tick: std::time::Instant::now(), interval: Duration::ZERO, armed: false }
    }
    /// The tick to serve on, armed by this ask. An armed one passed by more than half an interval
    /// is not a wake's lateness but an idle stretch, and the next tick takes its place.
    pub(crate) fn due(&mut self, interval: Duration, now: std::time::Instant) -> std::time::Instant {
        if !self.armed || self.interval != interval || self.tick + interval / 2 <= now {
            self.tick = tick(interval, now);
            self.interval = interval;
            self.armed = true;
        }
        self.tick
    }
    /// A serve took this tick: the next one is nobody's until a wait arms it.
    pub(crate) fn take(&mut self, interval: Duration, now: std::time::Instant) {
        self.tick = tick(interval, now + Duration::from_nanos(1));
        self.armed = false;
    }
}

/// What a slot's loop shares with its map entry, which the loop knows its own entry by.
struct Slot {
    specs: Mutex<HashMap<ConnId, Declared>>,
    /// The variables that follow this slot, fed the RAW frame — never a reduction.
    taps: Mutex<Vec<Tap>>,
    /// The taps moved: the frame held is fed to them, as a producer at rest sends no other.
    retap: AtomicBool,
    /// `Bytes` so the socket task forwards the SHARED buffer — a per-subscriber copy undoes dedup.
    tx: broadcast::Sender<Bytes>,
    stop: AtomicBool,
    reductions: AtomicU64,
    /// Serve generation: bumped on every spec change, join and leave, so the loop re-serves the
    /// current frame once even when the producer has not emitted or the frame did not change.
    gen: AtomicU64,
    /// The latest frame as it arrived — what serves a re-attaching viewer, and what the variables
    /// following this slot read. Reduced only where nothing but viewers is watching.
    latest: Mutex<Option<goofi_core::Data>>,
    /// A `node snapshot` asked, so the feed is wanted RAW even with no viewer. Never set at birth,
    /// which would spend a full-resolution frame on every slot the first time it is watched.
    asked: AtomicBool,
}

struct SlotReducer {
    slot: Arc<Slot>,
    /// The bridge's bell on the slot's view door — the same door the producer rings once a frame
    /// is out. A joiner, a spec, a tap, an ask, a settle and the stop all ring it; nothing polls.
    bell: Option<goofi_transport::Doorbell>,
    /// The loop's thread, joined at shutdown.
    worker: Option<goofi_supervisor::worker::Worker>,
}

impl SlotReducer {
    fn poke(&self) {
        if let Some(bell) = &self.bell {
            let _ = bell.ring(goofi_transport::VIEW_POKE_ID);
        }
    }

    /// Owe the subscribers one more serve of the current frame.
    fn reserve(&self) {
        self.slot.gen.fetch_add(1, Ordering::Release);
        self.poke();
    }
}

impl Drop for SlotReducer {
    fn drop(&mut self) {
        // Signalled, never joined: the slot's own loop is what removes its entry, and a join
        // from there would be a join on itself. The stop is set first, then the loop is rung.
        self.slot.stop.store(true, Ordering::Relaxed);
        self.poke();
    }
}

/// Every per-slot reducer, one per watched `(node, slot)`; cloneable, sharing one inner map.
#[derive(Clone)]
pub struct SlotReducers {
    inner: Arc<Mutex<HashMap<SlotKey, SlotReducer>>>,
    graph: Arc<Mutex<Graph>>,
    next_conn: Arc<AtomicU64>,
    /// One iceoryx2 node behind every feed: the reducers are ONE port owner, and a node per feed
    /// made each viewer's attach mint a node directory — a create that can fail hard on Windows.
    iox: SharedIox,
    /// Where every tap's pick goes: the follower, which writes the variable and broadcasts.
    follow: std::sync::mpsc::Sender<Followed>,
    /// The graph's instance, which every view door is named under.
    instance: Arc<str>,
    /// The store, read for the viewer cap; and the producer, rung for the frame a fresh feed is owed.
    store: Arc<Mutex<goofi_core::variables::VariableStore>>,
    variables: Arc<crate::variables::Variables>,
}

impl SlotReducers {
    pub fn new(iox: Arc<goofi_transport::Iox>, graph: Arc<Mutex<Graph>>, variables: Arc<crate::variables::Variables>, follow: std::sync::mpsc::Sender<Followed>) -> SlotReducers {
        let (instance, store) = {
            let g = graph.lock();
            (Arc::from(g.instance()), g.variable_store())
        };
        SlotReducers {
            inner: Arc::new(Mutex::new(HashMap::new())),
            graph,
            next_conn: Arc::new(AtomicU64::new(1)),
            iox: Arc::new((iox, Mutex::new(None))),
            follow,
            instance,
            store,
            variables,
        }
    }

    /// The gap the viewer cap asks for between two writes of one stream.
    pub fn cap_interval(&self) -> Duration {
        Duration::from_secs_f64(1.0 / cap_of(&self.store.lock()))
    }

    /// The graph settled: every loop re-reads its slot's address on its next wake, so a restart
    /// re-homes the feed and a removed node ends its reducer.
    pub fn poke_all(&self) {
        for reducer in self.inner.lock().values() {
            reducer.poke();
        }
    }

    /// Hand the follower what settled state says it computes; the taps below feed it.
    pub fn follow(&self, msg: Followed) {
        let _ = self.follow.send(msg);
    }

    /// Declare every followed slot at once, from settled state: a slot in `taps` feeds its
    /// variables from here on, and every other slot feeds none.
    pub fn set_taps(&self, taps: HashMap<SlotKey, Vec<Tap>>) {
        let mut map = self.inner.lock();
        for (key, reducer) in map.iter() {
            if !taps.contains_key(key) {
                reducer.slot.taps.lock().clear();
                reducer.poke();
            }
        }
        for (key, list) in taps {
            let reducer = self.ensure(&mut map, &key);
            *reducer.slot.taps.lock() = list;
            reducer.slot.retap.store(true, Ordering::Release);
            reducer.poke();
        }
    }

    /// Stop every loop and wait for each within one deadline.
    pub fn stop_all(&self, within: Duration) {
        let reducers: Vec<SlotReducer> = self.inner.lock().drain().map(|(_, r)| r).collect();
        for reducer in &reducers {
            reducer.slot.stop.store(true, Ordering::Relaxed);
            reducer.poke();
        }
        let deadline = std::time::Instant::now() + within;
        for mut reducer in reducers {
            if let Some(worker) = reducer.worker.take() {
                worker.join_within(deadline.saturating_duration_since(std::time::Instant::now()));
            }
        }
    }

    /// A fresh connection id.
    pub fn new_conn(&self) -> ConnId {
        self.next_conn.fetch_add(1, Ordering::Relaxed)
    }

    /// The slot's reducer, spawning its task if it has none.
    fn ensure<'a>(
        &self,
        map: &'a mut HashMap<SlotKey, SlotReducer>,
        key: &SlotKey,
    ) -> &'a mut SlotReducer {
        if map.get(key).is_some_and(|r| r.slot.stop.load(Ordering::Relaxed)) {
            map.remove(key);
        }
        map.entry(key.clone()).or_insert_with(|| {
            let door = goofi_transport::view_door_service(&self.instance, key.0, &key.1);
            let bell = shared_iox(&self.iox).and_then(|n| goofi_transport::Doorbell::open(&n, &door).ok());
            let slot = Slot {
                specs: Mutex::new(HashMap::new()),
                taps: Mutex::new(Vec::new()),
                retap: AtomicBool::new(false),
                tx: broadcast::channel(16).0,
                // No door is no wake: the stream is closed, and the next request tries again.
                stop: AtomicBool::new(bell.is_none()),
                reductions: AtomicU64::new(0),
                gen: AtomicU64::new(0),
                latest: Mutex::new(None),
                asked: AtomicBool::new(false),
            };
            let mut reducer = SlotReducer { slot: Arc::new(slot), bell, worker: None };
            if reducer.bell.is_some() {
                reducer.worker = spawn_reducer(self, key.clone(), &reducer, door);
            }
            reducer
        })
    }

    /// Subscribe `conn` to `key`'s reduced stream, spawning the slot's reducer task if it has none.
    pub fn subscribe(&self, key: SlotKey, conn: ConnId) -> broadcast::Receiver<Bytes> {
        let mut map = self.inner.lock();
        let reducer = self.ensure(&mut map, &key);
        if reducer.slot.stop.load(Ordering::Relaxed) {
            // A failed spawn closes this stream; the next request can try again.
            return broadcast::channel(1).1;
        }
        reducer.slot.specs.lock().entry(conn).or_default();
        // The receiver MUST exist before the bump, or a sweep between the two statements
        // broadcasts the join-serve into a fan-out this joiner is not yet part of.
        let rx = reducer.slot.tx.subscribe();
        reducer.reserve();
        rx
    }

    /// The slot's latest frame when it arrived at full resolution — never a producer's reduction,
    /// never a subscription. Asking widens the demand, so an asker that polls gets one.
    pub fn latest(&self, key: SlotKey) -> Option<goofi_core::Data> {
        // The `inner` guard is released before `latest` is taken, mirroring the reducer's order.
        let slot = {
            let mut map = self.inner.lock();
            let r = self.ensure(&mut map, &key);
            r.slot.asked.store(true, Ordering::Release);
            r.poke();
            r.slot.clone()
        };
        let frame = slot.latest.lock().clone().filter(|d| !is_reduced(d));
        frame
    }

    /// Replace what `conn` declares for `key` (latest-wins). No-op if the slot is gone.
    pub fn declare(&self, key: &SlotKey, conn: ConnId, declared: Declared) {
        if let Some(r) = self.inner.lock().get(key) {
            r.slot.specs.lock().insert(conn, declared);
            r.reserve();
        }
    }

    /// Ask the slot to serve its current frame again, for a connection that DROPPED one; it
    /// reaches every subscriber, which is one duplicate frame on a rare path.
    pub fn reoffer(&self, key: &SlotKey) {
        if let Some(r) = self.inner.lock().get(key) {
            r.reserve();
        }
    }

    /// Withdraw `conn` from `key`'s union; the reducer STAYS, rung so an unread feed starts its idle
    /// clock, and serves the frame once more under the plan that remains.
    pub fn unsubscribe(&self, key: &SlotKey, conn: ConnId) {
        if let Some(r) = self.inner.lock().get(key) {
            r.slot.specs.lock().remove(&conn);
            r.reserve();
        }
    }

    /// Number of live slot reducers (test/diagnostic).
    pub fn active_slots(&self) -> usize {
        self.inner.lock().len()
    }

    /// Subscribers on a slot (test/diagnostic).
    pub fn subscribers(&self, key: &SlotKey) -> usize {
        self.inner.lock().get(key).map(|r| r.slot.specs.lock().len()).unwrap_or(0)
    }

    /// Reduce+encode passes run for a slot so far (test/diagnostic).
    pub fn reductions(&self, key: &SlotKey) -> u64 {
        self.inner.lock().get(key).map(|r| r.slot.reductions.load(Ordering::Relaxed)).unwrap_or(0)
    }

    /// The iceoryx2 node every feed is built from, by id — `None` until the first feed mints it.
    /// One port owner is one node, so this never changes again (test/diagnostic).
    pub fn iox_node_id(&self) -> Option<u128> {
        self.iox.1.lock().as_ref().map(|n| n.id().value())
    }
}

/// How long a reducer nobody asks of keeps its subscription, and with it the producer's ring.
pub const IDLE: Duration = Duration::from_secs(1);
/// One wake after a watch begins: the producer takes the door on its own thread, so a frame it
/// published between the feed opening and that landing rang nobody, and is collected here.
const WATCH_GRACE: Duration = Duration::from_millis(250);

/// The reducers' ONE iceoryx2 node, minted on first feed and shared by every later one.
type SharedIox = Arc<(Arc<goofi_transport::Iox>, Mutex<Option<Arc<goofi_transport::IoxNode>>>)>;

/// The shared node, minting it if this is the first feed to ask; `None` is retried by the next.
fn shared_iox(iox: &SharedIox) -> Option<Arc<goofi_transport::IoxNode>> {
    let mut held = iox.1.lock();
    if held.is_none() {
        *held = iox.0.node().ok().map(Arc::new);
    }
    held.clone()
}

/// One end of a slot's data service: the subscriber and its iceoryx2 node.
struct SlotFeed {
    subscriber: goofi_transport::ByteSubscriber,
    /// Declared LAST: fields drop in order, and a node dropped before its subscriber cannot remove
    /// its own directory.
    _node: Arc<goofi_transport::IoxNode>,
}

/// Open a subscriber on `(uid, slot)`'s current output service, or `None` while the producer is
/// not addressable; a miss is retried on the next re-home rather than being fatal. A variable's
/// producer holds its last frame, and is poked for it once the feed is open.
fn open_feed(graph: &Mutex<Graph>, iox: &SharedIox, variables: &crate::variables::Variables, uid: Uid, slot: &str) -> Option<SlotFeed> {
    let service = {
        let g = graph.lock();
        crate::producer_alive(&g, uid, slot).then(|| crate::output_service_of(&g, uid, slot))?
    };
    let node = shared_iox(iox)?;
    let subscriber = goofi_transport::open_output_subscriber(&node, &service).ok()?;
    if uid == Uid::VARIABLES {
        variables.poke();
    }
    Some(SlotFeed { _node: node, subscriber })
}

/// Spawn the slot's loop on a plain thread, parked on the view door the producer and the bridge
/// ring; a held serve, an idle expiry, a watch's grace and a snapshot's window are its deadlines.
fn spawn_reducer(reducers: &SlotReducers, key: SlotKey, reducer: &SlotReducer, door: String) -> Option<goofi_supervisor::worker::Worker> {
    let (graph, iox, follow) = (reducers.graph.clone(), reducers.iox.clone(), reducers.follow.clone());
    let (store, variables) = (reducers.store.clone(), reducers.variables.clone());
    // Weak: the map owns this loop's entry, and the loop removes it; a strong one would be a cycle.
    let slots: Weak<Mutex<HashMap<SlotKey, SlotReducer>>> = Arc::downgrade(&reducers.inner);
    let (shared, failed) = (reducer.slot.clone(), reducer.slot.clone());
    let (uid, slot) = key.clone();
    match goofi_transport::thread(format!("goofi-reduce-{slot}")).spawn(move || {
        let Slot { specs, taps, retap, tx, stop, reductions, gen, latest, asked } = &*shared;
        // Owed a serve: watched, holding a frame, and with a new frame or a bump not yet served.
        let owed = |made: &Option<Bytes>, served: Option<u64>, pending: bool| {
            !specs.lock().is_empty() && (made.is_some() || latest.lock().is_some()) && (pending || served != Some(gen.load(Ordering::Acquire)))
        };
        let listener = shared_iox(&iox)
            .and_then(|n| goofi_transport::event_service(&n, &door).ok())
            .and_then(|d| d.listener_builder().create().ok());
        let Some(listener) = listener else {
            stop.store(true, Ordering::Relaxed);
            return;
        };
        let mut feed: Option<SlotFeed> = None;
        let time = graph.lock().time();
        // The cache still belongs to this service after an idle port is dropped.
        let mut source: Option<String> = None;
        // The graph epoch the address was last read at: a poke re-reads it only when the graph
        // moved since, or while the feed has yet to open.
        let epoch = graph.lock().epoch();
        let mut homed: Option<u64> = None;
        // Whether the graph has been told this slot is watched — the producer's ring follows it —
        // and the one wake a fresh watch owes itself.
        let mut watching = false;
        let mut grace: Option<std::time::Instant> = None;
        // An attached subscriber is what a scheduled engine reads as demand, so a feed nobody
        // wants keeps a GPU node rendering for ever.
        let mut asked_at = std::time::Instant::now();
        let mut wanted = true;
        // `served: None` means "never broadcast", which is what sends the first frame without a bump.
        let mut served: Option<u64> = None;
        let mut pending = false;
        let mut pace = Pace::new();
        // What the producer was last told its readers want. Pushed only on a CHANGE: it takes the
        // graph lock, and a viewer's box moves rarely — the frontend quantizes it to 32-px steps.
        let mut demanded: Option<Option<goofi_view::ViewWant>> = None;
        // The last frame's kind and shape, off its header — what the planner reads for a frame
        // this loop never decoded. `made` is that frame itself, ready to forward as it stands.
        let mut peeked: Option<Peek> = None;
        let mut made: Option<Bytes> = None;
        // A snapshot holds the demand wide for IDLE after the last ask, so an asker that polls
        // finds the full frame its first ask made the producer send.
        let mut snapped: Option<std::time::Instant> = None;
        // What the raw frame last reduced and served SAID, so one that says it again is not sent.
        let mut sent: Option<u64> = None;
        let mut stamped: Option<u64> = None;
        loop {
            let now = std::time::Instant::now();
            let interval = serve_interval(&specs.lock(), cap_of(&store.lock()));
            // Armed only when a serve is owed: a fresh frame after an idle tick waits for the next.
            // A tick already passed stays owed, so the pass after this wait serves it.
            let duties = [
                owed(&made, served, pending).then(|| pace.due(interval, now)),
                (feed.is_some() && !wanted).then_some(asked_at + IDLE),
                grace.filter(|at| *at > now),
                snapped.map(|at| at + IDLE).filter(|at| *at > now),
            ];
            let mut poked = false;
            let mut note = |id: goofi_transport::WakeId| poked |= id.as_value() == goofi_transport::VIEW_POKE_ID as usize;
            match duties.into_iter().flatten().min() {
                Some(at) => goofi_transport::wait_within(&listener, at.saturating_duration_since(now), &mut note),
                None => {
                    let _ = listener.blocking_wait_all(&mut note);
                }
            }
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if grace.is_some_and(|at| std::time::Instant::now() >= at) {
                grace = None;
            }
            // Read ONCE: the swap consumes it, so a second reader downstream would always miss.
            let snapshot = asked.swap(false, Ordering::Acquire);
            if snapshot {
                snapped = Some(std::time::Instant::now());
            }
            let full_res = snapped.is_some_and(|at| at.elapsed() < IDLE);
            wanted = full_res || !specs.lock().is_empty() || !taps.lock().is_empty();
            if wanted {
                asked_at = std::time::Instant::now();
            }
            // The graph may have moved: the slot's service carries the node's GENERATION, so a
            // restart re-homes the feed, and the node leaving the graph is the reducer's ONE death.
            let seen = epoch.load(Ordering::Acquire);
            if homed.is_none() || (poked && (feed.is_none() || homed != Some(seen))) {
                homed = Some(seen);
                let current = {
                    let g = graph.lock();
                    crate::producer_alive(&g, uid, &slot).then(|| crate::output_service_of(&g, uid, &slot))
                };
                let Some(current) = current else {
                    if let Some(slots) = slots.upgrade() {
                        // Only THIS task's entry: an undo puts a removed node back at the same uid,
                        // and a viewer that re-subscribed since holds a reducer this one must keep.
                        let mut map = slots.lock();
                        if map.get(&key).is_some_and(|r| Arc::ptr_eq(&r.slot, &shared)) {
                            map.remove(&key);
                        }
                    }
                    return;
                };
                if source.as_ref() != Some(&current) {
                    source = Some(current);
                    // A new generation is a new producer, and its readback starts at the frame's
                    // own size — so what this loop believes it asked for holds nowhere any more.
                    demanded = None;
                    peeked = None;
                    made = None;
                    *latest.lock() = None;
                    pending = false;
                    served = None;
                    sent = None;
                    stamped = None;
                    feed = None;
                }
            }
            if asked_at.elapsed() > IDLE {
                feed = None;
            } else if feed.is_none() {
                feed = open_feed(&graph, &iox, &variables, uid, &slot);
            }
            // Watched exactly while the feed is open: the producer rings this door once each
            // frame is out, and stops when nobody reads them.
            if watching != feed.is_some() {
                watching = feed.is_some();
                grace = watching.then(|| std::time::Instant::now() + WATCH_GRACE);
                let mut g = graph.lock();
                if g.set_view_watch(uid, &slot, watching) {
                    g.settle();
                }
            }
            let mut fresh = false;
            if let Some(f) = &feed {
                while let Ok(Some(sample)) = f.subscriber.receive() {
                    let Some(header) = Peek::of(sample.payload()) else { continue };
                    // A frame already in the viewers' own width is FORWARDED: decoding it to f32 only
                    // to quantize it back is the cost the demand exists to remove.
                    if header.ready {
                        peeked = Some(header);
                        *latest.lock() = None;
                        made = Some(Bytes::copy_from_slice(sample.payload()));
                        fresh = true;
                        continue;
                    }
                    if let Ok(frame) = goofi_codec::decode(sample.payload()) {
                        peeked = Some(header);
                        made = None;
                        *latest.lock() = Some(frame);
                        fresh = true;
                    }
                }
            }
            pending |= fresh;
            if fresh || retap.swap(false, Ordering::Acquire) {
                let taps = taps.lock().clone();
                if let (false, Some(d)) = (taps.is_empty(), latest.lock().clone()) {
                    // The frame as it came, the stamps off: a variable holds a value, not its tick.
                    let frame = match d.value() {
                        goofi_core::Value::Array(a) => goofi_core::Data::array(a.clone(), goofi_core::Meta::default()),
                        _ => d,
                    };
                    for tap in taps {
                        let _ = follow.send(Followed::Frame { variable: tap.variable, var: tap.var, frame: frame.clone() });
                    }
                }
            }
            // The raw frame for a variable or a snapshot, a box for a declared viewer, and one texel
            // for a reader that declared nothing — never the whole frame, an engine's dearest output.
            let want = if full_res || !taps.lock().is_empty() {
                None
            } else {
                // Planned against the frame's HEADER, so a frame this loop forwarded without
                // decoding still says what the viewers may ask of the one after it.
                peeked.as_ref().map_or(Some(goofi_view::UNDECLARED_WANT), |p| goofi_view::asked_box(&union_specs(&specs.lock()), p))
            };
            // Only while subscribed — the producer this names is the one the feed is open on — and
            // on a change alone, because it takes the graph lock.
            if feed.is_some() && demanded != Some(want) {
                demanded = Some(want);
                graph.lock().set_view_demand(uid, &slot, want);
            }
            // Nobody watching, nothing held, or nothing new: the cache keeps the frame for a return.
            if !owed(&made, served, pending) {
                continue;
            }
            let g_now = gen.load(Ordering::Acquire);
            // What a frame says and its stamps go each when its own hash moved, so a repeat goes as
            // stamps or not at all — unless a joiner, a leaver, a spec or a re-offer asked for it.
            let (hash, stamps) = match &made {
                Some(_) => (None, None),
                None => {
                    let held = latest.lock();
                    (held.as_ref().and_then(|d| goofi_codec::content_hash(d).ok()), held.as_ref().and_then(|d| goofi_codec::stamp_hash(d).ok()))
                }
            };
            let same = hash.is_some() && served == Some(g_now) && hash == sent;
            if same && stamps == stamped {
                pending = false;
                continue;
            }
            // The viewer rate, held here, where N viewers became one stream; a serve held back is
            // the duty the loop wakes to when the pace comes due. The rate is read again: the
            // viewer this serves may have joined during the wait.
            let now = std::time::Instant::now();
            let interval = serve_interval(&specs.lock(), cap_of(&store.lock()));
            if now < pace.due(interval, now) {
                continue;
            }
            let bytes = match &made {
                // Already exactly what the viewers asked for: one buffer, shared by every
                // subscriber, and no pass over a texel anywhere in this process.
                Some(ready) => ready.clone(),
                None => {
                    let Some(d) = latest.lock().clone() else { continue };
                    // A functional is a function of the time; the viewers see its value now.
                    let d = match (d.as_functional().is_some(), graph.lock().evaluator()) {
                        (false, _) => d,
                        (true, None) => continue,
                        (true, Some(ev)) => match goofi_node::at(&*ev, &d, time.now(), (0.0, 1.0)) {
                            Ok(v) => v,
                            Err(e) => {
                                goofi_supervisor::log::record(goofi_supervisor::log::Source::component("reducer"), goofi_supervisor::log::Level::Error, None, format!("{door}: {}", e.0));
                                continue;
                            }
                        },
                    };
                    let encoded = if same {
                        goofi_codec::encode_stamps(d.meta())
                    } else {
                        let specs = union_specs(&specs.lock());
                        // A table has no texels to plan; an array is reduced to its viewers' plan.
                        let (out, depth) = match d.value() {
                            goofi_core::Value::Table(_) => (goofi_core::reduce::reduce_table(&d, &specs), goofi_view::Depth::F32),
                            _ => {
                                let plan = goofi_view::plan(&specs, &d);
                                (goofi_core::reduce::reduce_for_view(&d, &plan), plan.depth)
                            }
                        };
                        reductions.fetch_add(1, Ordering::Relaxed);
                        // As narrow as the widest viewer of the slot draws; the reduction itself is
                        // f32 either way, and a half or a texel only for a frame they can hold.
                        let narrowed = match depth {
                            goofi_view::Depth::U8 => goofi_core::reduce::quantize_u8(&out)
                                .map(|(shape, texels, meta)| goofi_codec::encode_u8(&shape, &texels, &meta)).transpose(),
                            goofi_view::Depth::F16 => goofi_codec::encode_f16(&out),
                            goofi_view::Depth::F32 => Ok(None),
                        };
                        narrowed.and_then(|n| n.map_or_else(|| goofi_codec::encode(&out), Ok))
                    };
                    match encoded {
                        Ok(bytes) => Bytes::from(bytes),
                        Err(why) => {
                            goofi_supervisor::log::record(goofi_supervisor::log::Source::component("reducer"), goofi_supervisor::log::Level::Error, None, format!("{door}: the reduced frame cannot cross: {why}"));
                            continue;
                        }
                    }
                }
            };
            // The serve slot is taken by a frame served, never by one this tick held back.
            pace.take(interval, now);
            let _ = tx.send(bytes); // Err only if all receivers are momentarily gone — harmless.
            sent = hash;
            stamped = stamps;
            served = Some(g_now);
            pending = false;
        }
    }) {
        Ok(worker) => Some(worker),
        Err(error) => {
            failed.stop.store(true, Ordering::Relaxed);
            eprintln!("could not start slot reducer: {error}");
            None
        }
    }
}

/// Whether a producer shrank this frame on the way out, as its own meta records.
fn is_reduced(d: &goofi_core::Data) -> bool {
    !matches!(d.meta().reduced(), None | Some(goofi_core::MetaValue::Null))
}

/// A frame read off its HEADER alone: its kind, its shape, and whether its body is already the
/// 8-bit texels an image viewer draws, which the loop forwards rather than remakes.
struct Peek {
    tag: u8,
    shape: Vec<usize>,
    ready: bool,
}

impl goofi_view::Reducible for Peek {
    fn dtype_tag(&self) -> u8 {
        self.tag
    }
    fn shape(&self) -> &[usize] {
        &self.shape
    }
}

impl Peek {
    fn of(payload: &[u8]) -> Option<Peek> {
        let (tag, _, body) = goofi_codec::split_frame(payload).ok()?;
        let (dtype, shape) = match tag {
            0 => {
                let (dtype, shape, samples) = goofi_codec::array_head(body)?;
                if dtype == b"|u1" {
                    let len = shape.iter().try_fold(1usize, |n, &d| n.checked_mul(d))?;
                    if len != samples.len() { return None; }
                    goofi_codec::frame_meta(payload).ok()?;
                }
                (dtype, shape)
            },
            _ => (&b""[..], Vec::new()),
        };
        Some(Peek { tag, shape, ready: dtype == b"|u1" })
    }
}

