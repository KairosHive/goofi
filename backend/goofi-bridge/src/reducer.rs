//! Shared per-slot reducers: ONE reduction per watched `(node, slot)`, sized to the union of its
//! connections' specs and fanned out; it lives until its node leaves, and is woken, never clocked.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use axum::body::Bytes;
use goofi_graph::{Graph, Uid};
use goofi_view::ViewSpec;
use tokio::sync::broadcast;

/// The physical stream a reducer serves: a node uid + one of its output slot names.
pub type SlotKey = (Uid, String);
/// A unique id per `/data` connection, so its spec contribution can be tracked + removed.
pub type ConnId = u64;
/// One variable following this slot, and the number it reads out of each frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Tap {
    pub variable: String,
    pub index: Option<usize>,
}
/// What a tap delivers: the variable, and the value its frame held.
pub type Followed = (String, goofi_core::variables::VariableValue);

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

/// The gap between two serves of a slot: its fastest display's, never faster than the cap.
fn serve_interval(by_conn: &HashMap<ConnId, Declared>, cap: f64) -> Duration {
    let fps = by_conn.values().map(|d| d.fps.unwrap_or(cap)).fold(1.0, f64::max);
    Duration::from_secs_f64(1.0 / fps.min(cap))
}

/// The viewer cap `system.viewer_fps` holds, as frames a second no slower than one.
fn cap_of(g: &Graph) -> f64 {
    let fps = match g.variables().get("system.viewer_fps") {
        Some(goofi_core::variables::VariableValue::Float(f)) => *f,
        Some(goofi_core::variables::VariableValue::Int(i)) => *i as f64,
        _ => 0.0,
    };
    if fps.is_finite() { fps.max(1.0) } else { 1.0 }
}

/// A phase-locked rate: each slot is the last one's TARGET plus the interval, so the work done
/// between two slots does not stretch the period. A slot missed by a whole interval starts over.
pub(crate) struct Pace(std::time::Instant);

impl Pace {
    pub(crate) fn new() -> Pace {
        Pace(std::time::Instant::now())
    }
    pub(crate) fn due(&self) -> std::time::Instant {
        self.0
    }
    pub(crate) fn take(&mut self, interval: Duration, now: std::time::Instant) {
        self.0 += interval;
        if self.0 < now {
            self.0 = now + interval;
        }
    }
}

struct SlotReducer {
    specs: Arc<Mutex<HashMap<ConnId, Declared>>>,
    /// The variables that follow this slot, fed the RAW frame — never a reduction.
    taps: Arc<Mutex<Vec<Tap>>>,
    /// `Bytes` so the socket task forwards the SHARED buffer — a per-subscriber copy undoes dedup.
    tx: broadcast::Sender<Bytes>,
    stop: Arc<AtomicBool>,
    reductions: Arc<AtomicU64>,
    /// Serve generation: bumped on every spec change, join and leave, so the loop re-serves the
    /// current frame once even when the producer has not emitted or the frame did not change.
    gen: Arc<AtomicU64>,
    /// The latest frame as it arrived — what serves a re-attaching viewer, and what the variables
    /// following this slot read. Reduced only where nothing but viewers is watching.
    latest: Arc<Mutex<Option<goofi_core::Data>>>,
    /// A `node snapshot` asked, so the feed is wanted RAW even with no viewer. Never set at birth,
    /// which would spend a full-resolution frame on every slot the first time it is watched.
    asked: Arc<AtomicBool>,
    /// The bridge's bell on the slot's view door — the same door the producer rings once a frame
    /// is out. A joiner, a spec, a tap, an ask, a settle and the stop all ring it; nothing polls.
    bell: Option<goofi_transport::Doorbell>,
}

impl SlotReducer {
    fn poke(&self) {
        if let Some(bell) = &self.bell {
            let _ = bell.ring(goofi_transport::VIEW_POKE_ID);
        }
    }
}

impl Drop for SlotReducer {
    fn drop(&mut self) {
        // Signalled, never joined: the slot's own loop is what removes its entry, and a join
        // from there would be a join on itself. The stop is set first, then the loop is rung.
        self.stop.store(true, Ordering::Relaxed);
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
    /// `system.viewer_fps` as f64 bits, projected from settled state.
    cap: Arc<AtomicU64>,
}

impl SlotReducers {
    pub fn new(graph: Arc<Mutex<Graph>>, follow: std::sync::mpsc::Sender<Followed>) -> SlotReducers {
        let (instance, cap) = {
            let g = graph.lock().unwrap();
            (Arc::from(g.instance()), cap_of(&g))
        };
        SlotReducers {
            inner: Arc::new(Mutex::new(HashMap::new())),
            graph,
            next_conn: Arc::new(AtomicU64::new(1)),
            iox: Arc::new(Mutex::new(None)),
            follow,
            instance,
            cap: Arc::new(AtomicU64::new(cap.to_bits())),
        }
    }

    /// Take the viewer cap from settled state.
    pub fn set_cap(&self, g: &Graph) {
        self.cap.store(cap_of(g).to_bits(), Ordering::Relaxed);
    }

    /// The gap the viewer cap asks for between two writes of one stream.
    pub fn cap_interval(&self) -> Duration {
        Duration::from_secs_f64(1.0 / f64::from_bits(self.cap.load(Ordering::Relaxed)))
    }

    /// The graph settled: every loop re-reads its slot's address on its next wake, so a restart
    /// re-homes the feed and a removed node ends its reducer.
    pub fn poke_all(&self) {
        for reducer in self.inner.lock().unwrap().values() {
            reducer.poke();
        }
    }

    /// Declare every followed slot at once, from settled state: a slot in `taps` feeds its
    /// variables from here on, and every other slot feeds none.
    pub fn set_taps(&self, taps: HashMap<SlotKey, Vec<Tap>>) {
        let mut map = self.inner.lock().unwrap();
        for (key, reducer) in map.iter() {
            if !taps.contains_key(key) {
                reducer.taps.lock().unwrap().clear();
                reducer.poke();
            }
        }
        for (key, list) in taps {
            let reducer = self.ensure(&mut map, &key);
            *reducer.taps.lock().unwrap() = list;
            reducer.poke();
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
        if map.get(key).is_some_and(|r| r.stop.load(Ordering::Relaxed)) {
            map.remove(key);
        }
        map.entry(key.clone()).or_insert_with(|| {
            let door = goofi_transport::view_door_service(&self.instance, key.0, &key.1);
            let bell = shared_iox(&self.iox).and_then(|n| goofi_transport::Doorbell::open(&n, &door).ok());
            let reducer = SlotReducer {
                specs: Arc::new(Mutex::new(HashMap::new())),
                taps: Arc::new(Mutex::new(Vec::new())),
                tx: broadcast::channel(16).0,
                // No door is no wake: the stream is closed, and the next request tries again.
                stop: Arc::new(AtomicBool::new(bell.is_none())),
                reductions: Arc::new(AtomicU64::new(0)),
                gen: Arc::new(AtomicU64::new(0)),
                latest: Arc::new(Mutex::new(None)),
                asked: Arc::new(AtomicBool::new(false)),
                bell,
            };
            if reducer.bell.is_some() {
                spawn_reducer(self, key.clone(), &reducer, door);
            }
            reducer
        })
    }

    /// Subscribe `conn` to `key`'s reduced stream, spawning the slot's reducer task if it has none.
    pub fn subscribe(&self, key: SlotKey, conn: ConnId) -> broadcast::Receiver<Bytes> {
        let mut map = self.inner.lock().unwrap();
        let reducer = self.ensure(&mut map, &key);
        if reducer.stop.load(Ordering::Relaxed) {
            // A failed spawn closes this stream; the next request can try again.
            return broadcast::channel(1).1;
        }
        reducer.specs.lock().unwrap().entry(conn).or_default();
        // The receiver MUST exist before the bump, or a sweep between the two statements
        // broadcasts the join-serve into a fan-out this joiner is not yet part of.
        let rx = reducer.tx.subscribe();
        reducer.gen.fetch_add(1, Ordering::Release);
        reducer.poke();
        rx
    }

    /// The slot's latest frame when it arrived at full resolution — never a producer's reduction,
    /// never a subscription. Asking widens the demand, so an asker that polls gets one.
    pub fn latest(&self, key: SlotKey) -> Option<goofi_core::Data> {
        // The `inner` guard is released before `latest` is taken, mirroring the reducer's order.
        let latest = {
            let mut map = self.inner.lock().unwrap();
            let r = self.ensure(&mut map, &key);
            r.asked.store(true, Ordering::Release);
            r.poke();
            r.latest.clone()
        };
        let frame = latest.lock().unwrap().clone().filter(|d| !is_reduced(d));
        frame
    }

    /// Replace what `conn` declares for `key` (latest-wins). No-op if the slot is gone.
    pub fn declare(&self, key: &SlotKey, conn: ConnId, declared: Declared) {
        if let Some(r) = self.inner.lock().unwrap().get(key) {
            r.specs.lock().unwrap().insert(conn, declared);
            r.gen.fetch_add(1, Ordering::Release);
            r.poke();
        }
    }

    /// Ask the slot to serve its current frame again, for a connection that DROPPED one; it
    /// reaches every subscriber, which is one duplicate frame on a rare path.
    pub fn reoffer(&self, key: &SlotKey) {
        if let Some(r) = self.inner.lock().unwrap().get(key) {
            r.gen.fetch_add(1, Ordering::Release);
            r.poke();
        }
    }

    /// Withdraw `conn` from `key`'s union; the reducer STAYS, rung so an unread feed starts its idle
    /// clock, and serves the frame once more under the plan that remains.
    pub fn unsubscribe(&self, key: &SlotKey, conn: ConnId) {
        if let Some(r) = self.inner.lock().unwrap().get(key) {
            r.specs.lock().unwrap().remove(&conn);
            r.gen.fetch_add(1, Ordering::Release);
            r.poke();
        }
    }

    /// Number of live slot reducers (test/diagnostic).
    pub fn active_slots(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    /// Subscribers on a slot (test/diagnostic).
    pub fn subscribers(&self, key: &SlotKey) -> usize {
        self.inner.lock().unwrap().get(key).map(|r| r.specs.lock().unwrap().len()).unwrap_or(0)
    }

    /// Reduce+encode passes run for a slot so far (test/diagnostic).
    pub fn reductions(&self, key: &SlotKey) -> u64 {
        self.inner.lock().unwrap().get(key).map(|r| r.reductions.load(Ordering::Relaxed)).unwrap_or(0)
    }

    /// The iceoryx2 node every feed is built from, by id — `None` until the first feed mints it.
    /// One port owner is one node, so this never changes again (test/diagnostic).
    pub fn iox_node_id(&self) -> Option<u128> {
        self.iox.lock().unwrap().as_ref().map(|n| n.id().value())
    }
}

/// How long a reducer nobody asks of keeps its subscription, and with it the producer's ring.
pub const IDLE: Duration = Duration::from_secs(1);
/// One wake after a watch begins: the producer takes the door on its own thread, so a frame it
/// published between the feed opening and that landing rang nobody, and is collected here.
const WATCH_GRACE: Duration = Duration::from_millis(250);

/// The reducers' ONE iceoryx2 node, minted on first feed and shared by every later one.
type SharedIox = Arc<Mutex<Option<Arc<goofi_transport::IoxNode>>>>;

/// The shared node, minting it if this is the first feed to ask; `None` is retried by the next.
fn shared_iox(iox: &SharedIox) -> Option<Arc<goofi_transport::IoxNode>> {
    let mut held = iox.lock().unwrap();
    if held.is_none() {
        *held = goofi_transport::iox_node().ok().map(Arc::new);
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

/// Open a subscriber on `(uid, slot)`'s current output service, or `None` while the node is not
/// addressable; a miss is retried on the next re-home rather than being fatal.
fn open_feed(graph: &Mutex<Graph>, iox: &SharedIox, uid: Uid, slot: &str) -> Option<SlotFeed> {
    let service = {
        let g = graph.lock().unwrap();
        g.manifest(uid)?;
        crate::output_service_of(&g, uid, slot)
    };
    let node = shared_iox(iox)?;
    let subscriber = goofi_transport::open_output_subscriber(&node, &service).ok()?;
    Some(SlotFeed { _node: node, subscriber })
}

/// Spawn the slot's loop on a plain thread, parked on the view door the producer and the bridge
/// ring; a held serve, an idle expiry, a watch's grace and a snapshot's window are its deadlines.
fn spawn_reducer(reducers: &SlotReducers, key: SlotKey, reducer: &SlotReducer, door: String) {
    let (graph, iox, follow) = (reducers.graph.clone(), reducers.iox.clone(), reducers.follow.clone());
    let cap = reducers.cap.clone();
    // Weak: the map owns this loop's entry, and the loop removes it; a strong one would be a cycle.
    let slots: Weak<Mutex<HashMap<SlotKey, SlotReducer>>> = Arc::downgrade(&reducers.inner);
    let (specs, tx, taps) = (reducer.specs.clone(), reducer.tx.clone(), reducer.taps.clone());
    let (reductions, gen) = (reducer.reductions.clone(), reducer.gen.clone());
    let (latest, stop) = (reducer.latest.clone(), reducer.stop.clone());
    let asked = reducer.asked.clone();
    let (uid, slot) = key.clone();
    let failed = stop.clone();
    if let Err(error) = goofi_transport::thread(format!("goofi-reduce-{slot}")).spawn(move || {
        let listener = shared_iox(&iox)
            .and_then(|n| goofi_transport::event_service(&n, &door).ok())
            .and_then(|d| d.listener_builder().create().ok());
        let Some(listener) = listener else {
            stop.store(true, Ordering::Relaxed);
            return;
        };
        let mut feed: Option<SlotFeed> = None;
        // The cache still belongs to this service after an idle port is dropped.
        let mut source: Option<String> = None;
        // The graph epoch the address was last read at: a poke re-reads it only when the graph
        // moved since, or while the feed has yet to open.
        let epoch = graph.lock().unwrap().epoch();
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
            let owed = {
                let watched = !specs.lock().unwrap().is_empty();
                let held = made.is_some() || latest.lock().unwrap().is_some();
                watched && held && (pending || served != Some(gen.load(Ordering::Acquire)))
            };
            let duties = [
                (owed && pace.due() > now).then_some(pace.due()),
                (feed.is_some() && !wanted).then_some(asked_at + IDLE),
                grace.filter(|at| *at > now),
                snapped.map(|at| at + IDLE).filter(|at| *at > now),
            ];
            let mut poked = false;
            let mut note = |id: goofi_transport::WakeId| poked |= id.as_value() == goofi_transport::VIEW_POKE_ID as usize;
            let _ = match duties.into_iter().flatten().min() {
                Some(at) => listener.timed_wait_all(&mut note, at.saturating_duration_since(now)),
                None => listener.blocking_wait_all(&mut note),
            };
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
            wanted = full_res || !specs.lock().unwrap().is_empty() || !taps.lock().unwrap().is_empty();
            if wanted {
                asked_at = std::time::Instant::now();
            }
            // The graph may have moved: the slot's service carries the node's GENERATION, so a
            // restart re-homes the feed, and the node leaving the graph is the reducer's ONE death.
            let seen = epoch.load(Ordering::Acquire);
            if homed.is_none() || (poked && (feed.is_none() || homed != Some(seen))) {
                homed = Some(seen);
                let current = {
                    let g = graph.lock().unwrap();
                    g.manifest(uid).map(|_| crate::output_service_of(&g, uid, &slot))
                };
                let Some(current) = current else {
                    if let Some(slots) = slots.upgrade() {
                        // Only THIS task's entry: an undo puts a removed node back at the same uid,
                        // and a viewer that re-subscribed since holds a reducer this one must keep.
                        let mut map = slots.lock().unwrap();
                        if map.get(&key).is_some_and(|r| Arc::ptr_eq(&r.specs, &specs)) {
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
                    *latest.lock().unwrap() = None;
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
                feed = open_feed(&graph, &iox, uid, &slot);
            }
            // Watched exactly while the feed is open: the producer rings this door once each
            // frame is out, and stops when nobody reads them.
            if watching != feed.is_some() {
                watching = feed.is_some();
                grace = watching.then(|| std::time::Instant::now() + WATCH_GRACE);
                let mut g = graph.lock().unwrap();
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
                        *latest.lock().unwrap() = None;
                        made = Some(Bytes::copy_from_slice(sample.payload()));
                        fresh = true;
                        continue;
                    }
                    if let Ok(frame) = goofi_codec::decode(sample.payload()) {
                        peeked = Some(header);
                        made = None;
                        *latest.lock().unwrap() = Some(frame);
                        fresh = true;
                    }
                }
            }
            pending |= fresh;
            if fresh {
                let taps = taps.lock().unwrap().clone();
                if let (false, Some(d)) = (taps.is_empty(), latest.lock().unwrap().clone()) {
                    for tap in taps {
                        if let Some(v) = pick(&d, tap.index) {
                            let _ = follow.send((tap.variable, v));
                        }
                    }
                }
            }
            // The raw frame for a variable or a snapshot, a box for a declared viewer, and one texel
            // for a reader that declared nothing — never the whole frame, an engine's dearest output.
            let want = if full_res || !taps.lock().unwrap().is_empty() {
                None
            } else {
                let held = specs.lock().unwrap();
                // Planned against the frame's HEADER, so a frame this loop forwarded without
                // decoding still says what the viewers may ask of the one after it.
                match peeked.as_ref() {
                    Some(p) if !held.values().all(|v| v.specs.is_empty()) => {
                        goofi_view::image_box(&union_specs(&held), p)
                    }
                    // Nothing declared, or nothing produced yet to measure a declaration against.
                    _ => Some(goofi_view::ViewWant {
                        size: goofi_view::UNDECLARED_BOX,
                        depth: goofi_view::Depth::F32,
                    }),
                }
            };
            // Only while subscribed — the producer this names is the one the feed is open on — and
            // on a change alone, because it takes the graph lock.
            if feed.is_some() && demanded != Some(want) {
                demanded = Some(want);
                graph.lock().unwrap().set_view_demand(uid, &slot, want);
            }
            // Nobody is watching: the cache holds the last frame for whoever returns.
            if specs.lock().unwrap().is_empty() {
                continue;
            }
            if made.is_none() && latest.lock().unwrap().is_none() {
                continue;
            }
            let g_now = gen.load(Ordering::Acquire);
            if !pending && served == Some(g_now) {
                continue; // nothing new to say — no emit, no joiner, no spec change
            }
            // What a frame says and its stamps go each when its own hash moved, so a repeat goes as
            // stamps or not at all — unless a joiner, a leaver, a spec or a re-offer asked for it.
            let (hash, stamps) = match &made {
                Some(_) => (None, None),
                None => {
                    let held = latest.lock().unwrap();
                    (held.as_ref().map(goofi_codec::content_hash), held.as_ref().map(goofi_codec::stamp_hash))
                }
            };
            let same = hash.is_some() && served == Some(g_now) && hash == sent;
            if same && stamps == stamped {
                pending = false;
                continue;
            }
            // The viewer rate, held here, where N viewers became one stream; a serve held back is
            // the duty the loop wakes to when the pace comes due.
            let now = std::time::Instant::now();
            if now < pace.due() {
                continue;
            }
            let bytes = match &made {
                // Already exactly what the viewers asked for: one buffer, shared by every
                // subscriber, and no pass over a texel anywhere in this process.
                Some(ready) => ready.clone(),
                None => {
                    let Some(d) = latest.lock().unwrap().clone() else { continue };
                    if same {
                        Bytes::from(goofi_codec::encode_stamps(d.meta()))
                    } else {
                        let specs = union_specs(&specs.lock().unwrap());
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
                                .map(|(shape, texels, meta)| goofi_codec::encode_u8(&shape, &texels, &meta)),
                            goofi_view::Depth::F16 => goofi_codec::encode_f16(&out),
                            goofi_view::Depth::F32 => None,
                        };
                        Bytes::from(narrowed.unwrap_or_else(|| goofi_codec::encode(&out)))
                    }
                }
            };
            // The serve slot is taken by a frame served, never by one this tick held back.
            pace.take(serve_interval(&specs.lock().unwrap(), f64::from_bits(cap.load(Ordering::Relaxed))), now);
            let _ = tx.send(bytes); // Err only if all receivers are momentarily gone — harmless.
            sent = hash;
            stamped = stamps;
            served = Some(g_now);
            pending = false;
        }
    }) {
        failed.store(true, Ordering::Relaxed);
        eprintln!("could not start slot reducer: {error}");
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
    fn ndim(&self) -> usize {
        self.shape.len()
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

/// The one number a tap reads out of a frame: the indexed element of an array, the only element
/// of a one-element array, or a string whole. A wider array with no index answers nothing.
fn pick(d: &goofi_core::Data, index: Option<usize>) -> Option<goofi_core::variables::VariableValue> {
    use goofi_core::variables::VariableValue;
    match d.value() {
        goofi_core::Value::Str(s) => Some(VariableValue::Str(s.to_string())),
        goofi_core::Value::Array(a) => {
            let bytes = a.as_bytes();
            let i = match index {
                Some(i) => i,
                None if bytes.len() == 4 => 0,
                None => return None,
            };
            let start = i.checked_mul(4)?;
            let chunk: [u8; 4] = bytes.get(start..start.checked_add(4)?)?.try_into().ok()?;
            Some(VariableValue::Float(f32::from_le_bytes(chunk) as f64))
        }
        _ => None,
    }
}
