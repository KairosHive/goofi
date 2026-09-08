//! Shared per-slot data reducers: ONE reduction per active `(node, slot)`, sized to the union of
//! every subscribing connection's `ViewSpec`s and fanned out over a broadcast.
//!
//! The SLOT owns the reducer's lifetime, not the socket count — it lives until its node leaves the
//! graph, because a closing socket is no evidence that a slot stopped being watched. Its
//! SUBSCRIPTION follows demand though: an attached subscriber is what a scheduled engine reads as
//! "somebody wants frames", so a reducer nobody asks of lets its feed go.

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
/// One global following this slot, and the number it reads out of each frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Tap {
    pub global: String,
    pub index: Option<usize>,
}
/// What a tap delivers: the global, and the value its frame held.
pub type Followed = (String, goofi_core::globals::GlobalValue);

/// Flatten every connection's `ViewSpec`s into the single list the planner merges; a slot nobody
/// has declared for folds to the undeclared preview rather than to the full frame.
fn union_specs(by_conn: &HashMap<ConnId, Vec<ViewSpec>>) -> Vec<ViewSpec> {
    let merged: Vec<ViewSpec> = by_conn.values().flatten().cloned().collect();
    if merged.is_empty() { vec![ViewSpec::undeclared()] } else { merged }
}

struct SlotReducer {
    specs: Arc<Mutex<HashMap<ConnId, Vec<ViewSpec>>>>,
    /// The globals that follow this slot, fed the RAW frame — never a reduction.
    taps: Arc<Mutex<Vec<Tap>>>,
    /// `Bytes` so the socket task forwards the SHARED buffer — a per-subscriber copy undoes dedup.
    tx: broadcast::Sender<Bytes>,
    stop: Arc<AtomicBool>,
    reductions: Arc<AtomicU64>,
    /// Serve generation: bumped on every spec change and subscriber join, so the loop re-serves
    /// the current frame once even when the producer has not emitted.
    gen: Arc<AtomicU64>,
    /// The latest frame as it arrived — what serves a re-attaching viewer, and what the globals
    /// following this slot read. Reduced only where nothing but viewers is watching.
    latest: Arc<Mutex<Option<goofi_core::Data>>>,
    /// The last frame that arrived at FULL resolution. A producer that shrinks its output for the
    /// viewers watching it would otherwise leave `latest` holding a preview, and `node snapshot`
    /// asks for the frame itself. `Data` is an `Arc`, so holding it costs a refcount.
    full: Arc<Mutex<Option<goofi_core::Data>>>,
    /// Somebody read `latest` — a `node snapshot` — so the feed is wanted even with no viewer, and
    /// wanted RAW. Never set at birth: warming a fresh reducer is `asked_at`'s job, and starting
    /// here would spend one full-resolution frame on every slot the first time it is watched.
    asked: Arc<AtomicBool>,
}

impl Drop for SlotReducer {
    fn drop(&mut self) {
        // Signalled, never joined: the slot's own loop is what removes its entry, and a join
        // from there would be a join on itself.
        self.stop.store(true, Ordering::Relaxed);
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
    /// Where every tap's pick goes: the follower, which writes the global and broadcasts.
    follow: std::sync::mpsc::Sender<Followed>,
}

impl SlotReducers {
    pub fn new(graph: Arc<Mutex<Graph>>, follow: std::sync::mpsc::Sender<Followed>) -> SlotReducers {
        SlotReducers {
            inner: Arc::new(Mutex::new(HashMap::new())),
            graph,
            next_conn: Arc::new(AtomicU64::new(1)),
            iox: Arc::new(Mutex::new(None)),
            follow,
        }
    }

    /// Declare every followed slot at once, from settled state: a slot in `taps` feeds its
    /// globals from here on, and every other slot feeds none.
    pub fn set_taps(&self, taps: HashMap<SlotKey, Vec<Tap>>) {
        let mut map = self.inner.lock().unwrap();
        for (key, reducer) in map.iter() {
            if !taps.contains_key(key) {
                reducer.taps.lock().unwrap().clear();
            }
        }
        for (key, list) in taps {
            let reducer = self.ensure(&mut map, &key);
            *reducer.taps.lock().unwrap() = list;
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
        let slots = Arc::downgrade(&self.inner);
        map.entry(key.clone()).or_insert_with(|| {
            let reducer = SlotReducer {
                specs: Arc::new(Mutex::new(HashMap::new())),
                taps: Arc::new(Mutex::new(Vec::new())),
                tx: broadcast::channel(16).0,
                stop: Arc::new(AtomicBool::new(false)),
                reductions: Arc::new(AtomicU64::new(0)),
                gen: Arc::new(AtomicU64::new(0)),
                latest: Arc::new(Mutex::new(None)),
                full: Arc::new(Mutex::new(None)),
                asked: Arc::new(AtomicBool::new(false)),
            };
            let (graph, iox) = (self.graph.clone(), self.iox.clone());
            spawn_reducer(key.clone(), &reducer, graph, iox, slots, self.follow.clone());
            reducer
        })
    }

    /// Subscribe `conn` to `key`'s reduced stream, spawning the slot's reducer task if it has none.
    pub fn subscribe(&self, key: SlotKey, conn: ConnId) -> broadcast::Receiver<Bytes> {
        let mut map = self.inner.lock().unwrap();
        let reducer = self.ensure(&mut map, &key);
        reducer.specs.lock().unwrap().entry(conn).or_default();
        // The receiver MUST exist before the bump, or a sweep between the two statements
        // broadcasts the join-serve into a fan-out this joiner is not yet part of.
        let rx = reducer.tx.subscribe();
        reducer.gen.fetch_add(1, Ordering::Release);
        rx
    }

    /// The slot's latest RAW frame, once — never reduced, never a subscription. Asking also makes
    /// sure the slot's reducer runs, so a never-watched slot starts warming on the first ask.
    pub fn latest(&self, key: SlotKey) -> Option<goofi_core::Data> {
        // The `inner` guard is released before `latest` is taken, mirroring the reducer's order.
        let (full, latest) = {
            let mut map = self.inner.lock().unwrap();
            let r = self.ensure(&mut map, &key);
            r.asked.store(true, Ordering::Release);
            (r.full.clone(), r.latest.clone())
        };
        let frame = full.lock().unwrap().clone();
        frame.or_else(|| latest.lock().unwrap().clone())
    }

    /// Replace `conn`'s declared specs for `key` (latest-wins). No-op if the slot is gone.
    pub fn set_specs(&self, key: &SlotKey, conn: ConnId, specs: Vec<ViewSpec>) {
        if let Some(r) = self.inner.lock().unwrap().get(key) {
            r.specs.lock().unwrap().insert(conn, specs);
            r.gen.fetch_add(1, Ordering::Release);
        }
    }

    /// Ask the slot to serve its current frame again, for a connection that DROPPED one; it
    /// reaches every subscriber, which is one duplicate frame on a rare path.
    pub fn reoffer(&self, key: &SlotKey) {
        if let Some(r) = self.inner.lock().unwrap().get(key) {
            r.gen.fetch_add(1, Ordering::Release);
        }
    }

    /// Withdraw `conn`'s contribution to `key`'s spec union; the reducer itself STAYS.
    pub fn unsubscribe(&self, key: &SlotKey, conn: ConnId) {
        if let Some(r) = self.inner.lock().unwrap().get(key) {
            r.specs.lock().unwrap().remove(&conn);
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

/// How often the slot's subscribe address is re-derived from the graph: a service name carries the
/// node's GENERATION, so a restart re-homes the stream to a name this task has never opened.
pub const REHOME_INTERVAL: Duration = Duration::from_secs(1);
/// How long a reducer nobody asks of keeps its subscription. WALL TIME, not a count of sleeps: a
/// platform whose sleep rounds up would otherwise hold on for as much longer.
pub const IDLE: Duration = Duration::from_secs(1);

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

/// One end of a slot's data service: the subscriber, its iceoryx2 node, and the service name it
/// was opened on.
struct SlotFeed {
    subscriber: goofi_transport::ByteSubscriber,
    service: String,
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
    Some(SlotFeed { _node: node, subscriber, service })
}

/// Spawn the per-slot reducer loop, on a PLAIN thread so any transport's thread can open one:
/// every ~16 ms take whatever the producer has published and — only when it emitted, a subscriber
/// joined, or the spec union changed — reduce, encode once, and broadcast to all. The sweep is a
/// sampling deadline, never a send cadence.
fn spawn_reducer(
    key: SlotKey,
    reducer: &SlotReducer,
    graph: Arc<Mutex<Graph>>,
    iox: SharedIox,
    slots: Weak<Mutex<HashMap<SlotKey, SlotReducer>>>,
    follow: std::sync::mpsc::Sender<Followed>,
) {
    let (specs, tx, taps) = (reducer.specs.clone(), reducer.tx.clone(), reducer.taps.clone());
    let (reductions, gen) = (reducer.reductions.clone(), reducer.gen.clone());
    let (latest, stop) = (reducer.latest.clone(), reducer.stop.clone());
    let full = reducer.full.clone();
    let asked = reducer.asked.clone();
    let (uid, slot) = key.clone();
    goofi_transport::thread(format!("goofi-reduce-{slot}")).spawn(move || {
        let mut feed = open_feed(&graph, &iox, uid, &slot);
        let mut rehomed = std::time::Instant::now();
        // An attached subscriber is what a scheduled engine reads as demand, so a feed nobody
        // wants keeps a GPU node rendering for ever.
        let mut asked_at = std::time::Instant::now();
        // `served: None` means "never broadcast", which is what sends the first frame without a bump.
        let mut served: Option<u64> = None;
        let mut next_serve = std::time::Instant::now();
        // What the producer was last told its readers want. Pushed only on a CHANGE: it takes the
        // graph lock, and a viewer's box moves rarely — the frontend quantizes it to 32-px steps.
        let mut demanded: Option<Option<goofi_view::ViewWant>> = None;
        // The last frame's kind and shape, off its header — what the planner reads for a frame
        // this loop never decoded. `made` is that frame itself, ready to forward as it stands.
        let mut peeked: Option<Peek> = None;
        let mut made: Option<Bytes> = None;
        // A snapshot needs ONE full-resolution frame, so the demand is held wide until one lands
        // rather than for the single sweep the ask was seen on — the producer needs a tick to answer.
        let mut full_res = false;
        loop {
            std::thread::sleep(crate::vocab::REDUCER_TICK);
            if stop.load(Ordering::Relaxed) {
                return;
            }
            // Read ONCE: the swap consumes it, so a second reader downstream would always miss.
            let snapshot = asked.swap(false, Ordering::Acquire);
            // A slot being narrowed holds a full frame from before the ask, and answering with it
            // would answer a question nobody asked. Cleared, so the ask waits for its own frame.
            if snapshot && demanded.is_some_and(|w| w.is_some()) {
                *full.lock().unwrap() = None;
            }
            full_res |= snapshot;
            let wanted = snapshot || !specs.lock().unwrap().is_empty() || !taps.lock().unwrap().is_empty();
            if wanted {
                asked_at = std::time::Instant::now();
            }
            if asked_at.elapsed() > IDLE {
                feed = None;
            }
            if rehomed.elapsed() >= REHOME_INTERVAL {
                rehomed = std::time::Instant::now();
                let current = {
                    let g = graph.lock().unwrap();
                    g.manifest(uid).map(|_| crate::output_service_of(&g, uid, &slot))
                };
                let Some(current) = current else {
                    // The node left the graph: the reducer's ONE death.
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
                if feed.as_ref().is_some_and(|f| f.service != current) {
                    // A new generation is a new producer, and its readback starts at the frame's
                    // own size — so what this loop believes it asked for holds nowhere any more.
                    demanded = None;
                }
                if asked_at.elapsed() <= IDLE && feed.as_ref().is_none_or(|f| f.service != current) {
                    feed = open_feed(&graph, &iox, uid, &slot);
                }
            }
            if feed.is_none() && asked_at.elapsed() <= IDLE {
                feed = open_feed(&graph, &iox, uid, &slot);
            }
            let mut fresh = false;
            if let Some(f) = &feed {
                while let Ok(Some(sample)) = f.subscriber.receive() {
                    peeked = Peek::of(sample.payload()).or(peeked.take());
                    // A producer that answered the demand in the viewers' own width is FORWARDED:
                    // decoding those texels to f32 only to quantize them back is the whole cost the
                    // demand exists to remove, and there is nothing left here to reduce.
                    if peeked.as_ref().is_some_and(|p| p.ready) {
                        made = Some(Bytes::copy_from_slice(sample.payload()));
                        fresh = true;
                        continue;
                    }
                    made = None;
                    if let Ok(frame) = goofi_codec::decode(sample.payload()) {
                        if !is_reduced(&frame) {
                            *full.lock().unwrap() = Some(frame.clone());
                        }
                        *latest.lock().unwrap() = Some(frame);
                        fresh = true;
                    }
                }
            }
            if fresh {
                let taps = taps.lock().unwrap().clone();
                if let (false, Some(d)) = (taps.is_empty(), latest.lock().unwrap().clone()) {
                    for tap in taps {
                        if let Some(v) = pick(&d, tap.index) {
                            let _ = follow.send((tap.global, v));
                        }
                    }
                }
            }
            // The full-resolution frame a snapshot asked for has landed — which the FRAME says,
            // not the demand: the frame already in flight when the demand widened is a reduced one,
            // and taking it for the answer would end the ask before the producer had answered it.
            if fresh && full.lock().unwrap().is_some() {
                full_res = false;
            }
            // A demand is what a reader ASKED for: the raw frame for a global or a snapshot, and a
            // box for a declared viewer. A reader that declared nothing asked for no pixels, so it
            // gets one texel — never the whole frame, which is the most expensive thing an engine
            // can be told to make.
            let want = if full_res || !taps.lock().unwrap().is_empty() {
                None
            } else {
                let held = specs.lock().unwrap();
                // Planned against the frame's HEADER, so a frame this loop forwarded without
                // decoding still says what the viewers may ask of the one after it.
                match peeked.as_ref() {
                    Some(p) if !held.values().all(|v| v.is_empty()) => {
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
            if !fresh && served == Some(g_now) {
                continue; // nothing new to say — no emit, no joiner, no spec change
            }
            // The viewer rate, held HERE because this is the one place N viewers became one
            // stream. A producer emitting faster than the browser paints is bytes nobody draws.
            // The loop's own tick is the quantum, so the real ceiling is one tick coarser.
            let now = std::time::Instant::now();
            if now < next_serve {
                continue;
            }
            next_serve += crate::vocab::VIEWER_INTERVAL;
            if next_serve < now {
                next_serve = now + crate::vocab::VIEWER_INTERVAL;
            }
            let bytes = match &made {
                // Already exactly what the viewers asked for: one buffer, shared by every
                // subscriber, and no pass over a texel anywhere in this process.
                Some(ready) => ready.clone(),
                None => {
                    let Some(d) = latest.lock().unwrap().clone() else { continue };
                    let plan = goofi_view::plan(&union_specs(&specs.lock().unwrap()), &d);
                    let out = goofi_core::reduce::reduce_for_view(&d, &plan);
                    reductions.fetch_add(1, Ordering::Relaxed);
                    // 8-bit only where every viewer of the slot draws it, and only for a frame that
                    // has texels; the reduction itself is f32 either way.
                    let quantized = (plan.depth == goofi_view::Depth::U8)
                        .then(|| goofi_core::reduce::quantize_u8(&out))
                        .flatten()
                        .map(|(shape, texels, meta)| goofi_codec::encode_u8(&shape, &texels, &meta));
                    Bytes::from(quantized.unwrap_or_else(|| goofi_codec::encode(&out)))
                }
            };
            let _ = tx.send(bytes); // Err only if all receivers are momentarily gone — harmless.
            served = Some(g_now);
        }
    })
    .expect("the slot reducer thread");
}

/// Whether a producer shrank this frame on the way out, as its own meta records.
fn is_reduced(d: &goofi_core::Data) -> bool {
    !matches!(d.meta().reduced(), None | Some(goofi_core::MetaValue::Null))
}

/// A frame read off its HEADER alone — its kind, its shape, and whether its body is already the
/// 8-bit texels an image viewer draws. What a producer made to the plan is forwarded rather than
/// remade, so this is everything the loop knows about such a frame.
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
            0 => goofi_codec::array_head(body).map(|(d, shape, _)| (d, shape))?,
            _ => (&b""[..], Vec::new()),
        };
        Some(Peek { tag, shape, ready: dtype == b"|u1" })
    }
}

/// The one number a tap reads out of a frame: the indexed element of an array, the only element
/// of a one-element array, or a string whole. A wider array with no index answers nothing.
fn pick(d: &goofi_core::Data, index: Option<usize>) -> Option<goofi_core::globals::GlobalValue> {
    use goofi_core::globals::GlobalValue;
    match d.value() {
        goofi_core::Value::Str(s) => Some(GlobalValue::Str(s.to_string())),
        goofi_core::Value::Array(a) => {
            let bytes = a.as_bytes();
            let i = match index {
                Some(i) => i,
                None if bytes.len() == 4 => 0,
                None => return None,
            };
            let chunk: [u8; 4] = bytes.get(i * 4..i * 4 + 4)?.try_into().ok()?;
            Some(GlobalValue::Float(f32::from_le_bytes(chunk) as f64))
        }
        _ => None,
    }
}
