//! The recorder's drain: ONE thread that holds a subscriber on every armed stream, drains each to
//! exhaustion and writes what it takes straight to disk. Never the `/data` plane, which samples on
//! a timer and reduces for viewers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use goofi_core::time::Time;
use goofi_graph::{Graph, Uid};
use goofi_record::{Kind, Recorder, StreamId, StreamMeta, Timeline};
use goofi_transport::{record_shape, Halt};

/// The longest a sweep waits on the door — a CEILING on the park, never a cadence.
const WAKE: Duration = Duration::from_millis(20);

/// How often the armed set is re-derived from the graph. It is NOT done per wake: the wake rate is
/// the PUBLISH rate, and resolving there would hold the graph lock in a 32 kHz loop.
const RESOLVE: Duration = Duration::from_millis(25);

/// The engines whose armed slots publish GOOF frames on a record service, and how each one dates a
/// frame: a signal node reads the clock at its `process`, and the audio engine counts samples — so
/// one timeline carries the scheduler's jitter and the other cannot. Graphics is not here: it
/// writes to the recorder from its own render thread rather than over a service.
const DRAINED: &[(&str, Timeline)] = &[("signal", Timeline::Measured), ("audio", Timeline::Derived)];

fn timeline(engine: &str) -> Option<Timeline> {
    DRAINED.iter().find(|(e, _)| *e == engine).map(|(_, t)| *t)
}

struct Feed {
    id: StreamId,
    service: String,
    subscriber: goofi_transport::ByteSubscriber,
    last: Option<u64>,
    /// A failed open or write has been said on the stream and the feed is dead. Re-opening a file
    /// per frame against a full disk is worse than stopping.
    failed: bool,
    /// How this engine dates a frame, and how deep the recorder's end of its service is.
    timeline: Timeline,
    buffer: usize,
}

struct Drain {
    graph: Arc<Mutex<Graph>>,
    recorder: Arc<Recorder>,
    time: Arc<Time>,
    feeds: HashMap<(Uid, String), Feed>,
    listener: goofi_transport::Listener,
    /// Declared LAST: fields drop in order, and a node dropped before its ports cannot remove its
    /// own directory.
    node: goofi_transport::IoxNode,
}

/// Every armed signal stream the settled graph names, with the service its CURRENT generation
/// publishes on.
fn armed(g: &Graph) -> HashMap<(Uid, String), (String, StreamId)> {
    let mut out = HashMap::new();
    for uid in g.all_uids() {
        for slot in g.recorded(uid).unwrap_or(&[]) {
            let id = crate::arms::stream_id(g, uid, slot);
            if timeline(id.engine).is_none() {
                continue;
            }
            let base = goofi_transport::service_base(g.instance(), uid, g.node_generation(uid));
            out.insert((uid, slot.clone()), (goofi_transport::record_service(&base, slot), id));
        }
    }
    out
}

/// What one frame's meta says the stream it belongs to is. The timeline is the ENGINE's word, not
/// the frame's: a rate alone says nothing about whether the times were counted or read.
fn stream_meta(meta: Option<&goofi_core::Meta>, timeline: Timeline) -> StreamMeta {
    let sfreq = meta.and_then(|m| m.sfreq());
    let held = match timeline {
        Timeline::Measured => StreamMeta::measured(sfreq),
        Timeline::Derived => StreamMeta::derived(sfreq),
    };
    match meta.and_then(|m| m.channels().dims().next().map(|(_, c)| c.len())) {
        Some(n) => held.with_channels(n),
        None => held,
    }
}

/// Take every frame the feed holds, in one pass, and write each straight through. A recorder that
/// kept only the newest frame is the defect this feature exists to prevent, so nothing stops at one.
fn drain_feed(recorder: &Recorder, time: &Time, feed: &mut Feed) {
    if feed.failed {
        return;
    }
    let (mut taken, mut missed) = (0usize, 0u64);
    let mut drift: Option<f64> = None;
    while let Ok(Some(sample)) = feed.subscriber.receive() {
        let bytes = sample.payload();
        let meta = goofi_codec::frame_meta(bytes).ok();
        let at = meta.as_ref().and_then(|m| m.time()).unwrap_or_else(|| time.now());
        if !recorder.is_open(&feed.id) {
            if recorder.open(&feed.id, Kind::Frames, at, stream_meta(meta.as_ref(), feed.timeline)).is_err() {
                feed.failed = true;
                break;
            }
            feed.last = None;
        }
        if let Some(goofi_core::MetaValue::Float(d)) = meta.as_ref().and_then(|m| m.get(goofi_core::META_DRIFT)) {
            drift = Some(*d);
        }
        let index = meta.as_ref().and_then(|m| m.index());
        // The subscriber overflows the OLDEST frame and says nothing, so the indices are the only
        // witness. An index that RESETS is a rebirth, never a loss.
        let gap = match (index, feed.last) {
            (Some(i), Some(last)) => i.saturating_sub(last).saturating_sub(1),
            _ => 0,
        };
        if let Err(e) = recorder.write(&feed.id, bytes, gap, at) {
            recorder.close(&feed.id, &format!("the frame could not be written: {e}"));
            feed.failed = true;
            break;
        }
        feed.last = index;
        missed += gap;
        taken += 1;
    }
    if taken > 0 {
        recorder.fill(&feed.id, taken as f32 / feed.buffer as f32);
    }
    // Once per sweep, like the counts above: a derived timeline says how far it has walked from
    // patch time, and the manifest carries the last word.
    if let Some(d) = drift {
        recorder.drift(&feed.id, d);
    }
    // Once per sweep: every count already rode its own write, so only the manifest is left.
    if missed > 0 || taken > 0 {
        recorder.note();
    }
}

impl Drain {
    /// Reconcile the held feeds against the settled graph. A departing feed is drained TO
    /// EXHAUSTION before it is closed: what the service already delivered is transported data, and
    /// dropping the subscriber would lose it where no gap and no count could ever show it.
    fn resolve(&mut self) {
        let wanted = armed(&self.graph.lock().unwrap_or_else(|e| e.into_inner()));
        for key in self.feeds.keys().cloned().collect::<Vec<_>>() {
            let why = match wanted.get(&key) {
                Some((service, _)) if *service == self.feeds[&key].service => continue,
                Some(_) => "reborn",
                None => "disarmed",
            };
            let mut feed = self.feeds.remove(&key).expect("a key just read");
            drain_feed(&self.recorder, &self.time, &mut feed);
            self.recorder.close(&feed.id, why);
        }
        for (key, (service, id)) in wanted {
            if self.feeds.contains_key(&key) {
                continue;
            }
            let shape = record_shape(id.engine);
            let timeline = timeline(id.engine).expect("armed filtered the engines above");
            if let Ok(subscriber) = goofi_transport::open_record_subscriber(&self.node, &service, shape) {
                let feed =
                    Feed { id, service, subscriber, last: None, failed: false, timeline, buffer: shape.buffer };
                self.feeds.insert(key, feed);
            }
        }
    }

    fn sweep(&mut self) {
        for feed in self.feeds.values_mut() {
            drain_feed(&self.recorder, &self.time, feed);
        }
    }

    /// Every feed drained to exhaustion and let go. A stop asks for this before it closes a file,
    /// so the frames the last sweep did not reach are not the tail this recording loses.
    fn release(&mut self) {
        for (_, mut feed) in self.feeds.drain().collect::<Vec<_>>() {
            drain_feed(&self.recorder, &self.time, &mut feed);
        }
    }
}

/// Start the one drain. `halt` is what stops it, and what a teardown waits on to a ceiling.
pub fn spawn(graph: Arc<Mutex<Graph>>, recorder: Arc<Recorder>, halt: Arc<Halt>) {
    let (instance, time) = {
        let g = graph.lock().unwrap_or_else(|e| e.into_inner());
        (g.instance().to_string(), g.time())
    };
    let ran = halt.clone();
    let started = goofi_transport::thread("goofi-record").spawn(move || {
        let Ok(node) = goofi_transport::iox_node() else {
            halt.release();
            return;
        };
        let door = goofi_transport::event_service(&node, &goofi_transport::record_door_service(&instance));
        let listener = door.and_then(|d| d.listener_builder().create().map_err(|e| e.to_string()));
        let Ok(listener) = listener else {
            halt.release();
            return;
        };
        let mut drain = Drain { graph, recorder, time, feeds: HashMap::new(), listener, node };
        let mut resolved = Instant::now() - RESOLVE;
        while !halt.stopped() {
            // The event id is ignored, which spends none of the id budget: a burst across every
            // armed slot coalesces into one sweep.
            let _ = drain.listener.timed_wait_all(|_| {}, WAKE);
            // Read BEFORE the sweep: a sweep already under way when a stop asked is not an answer
            // to it.
            let mark = drain.recorder.sweeping();
            if !drain.recorder.running() {
                drain.release();
                drain.recorder.swept(mark);
                continue;
            }
            if resolved.elapsed() >= RESOLVE || drain.feeds.is_empty() {
                drain.resolve();
                resolved = Instant::now();
            }
            drain.sweep();
            drain.recorder.swept(mark);
        }
        drop(drain);
        halt.release();
    });
    if started.is_err() {
        // The thread is what releases the halt, so a teardown must not wait on one that never ran.
        ran.release();
    }
}
