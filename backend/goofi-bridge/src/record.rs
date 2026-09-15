//! The recorder's drain: ONE thread that holds a subscriber on every armed stream, drains each to
//! exhaustion and writes what it takes straight to disk. Never the `/data` plane, which samples on
//! a timer and reduces for viewers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use goofi_core::time::Time;
use goofi_graph::{Graph, Uid};
use goofi_record::{Recorder, StreamId, Timeline};
use goofi_transport::{record_shape, Halt};

/// The longest a sweep waits on the door — a CEILING on the park, never a cadence.
const WAKE: Duration = Duration::from_millis(20);


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
    /// The one frame the writer's lane refused, kept for the next sweep.
    held: Option<Vec<u8>>,
    /// How this engine dates a frame, and how deep the recorder's end of its service is.
    timeline: Timeline,
    /// Whether this engine's frames are AUDIO — the one stream kind that is a wav.
    rate: bool,
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

struct AudioCapture(std::sync::Weak<Mutex<Graph>>);

impl AudioCapture {
    fn flush(&self) -> Result<(), String> {
        let graph = self.0.upgrade().ok_or("the recording graph is gone")?;
        let waits = {
            let mut graph = graph.lock().unwrap_or_else(|e| e.into_inner());
            crate::try_audio_engine(&mut graph).map(|audio| audio.flush_recording()).unwrap_or_default()
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        for wait in waits {
            wait.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| "audio control did not acknowledge the recording flush")??;
        }
        Ok(())
    }
}

impl goofi_record::Capture for AudioCapture {
    fn prepare(&self) -> Result<(), String> {
        self.flush()
    }

    fn boundary(&self, window: Arc<goofi_core::record::FrameWindow>, begin: bool) -> Result<(), String> {
        let graph = self.0.upgrade().ok_or("the recording graph is gone")?;
        let action = {
            let mut graph = graph.lock().unwrap_or_else(|e| e.into_inner());
            crate::try_audio_engine(&mut graph).map(|audio| audio.recording_boundary(window, begin))
        };
        if let Some(action) = action {
            action()?;
        }
        if !begin {
            self.flush()?;
        }
        Ok(())
    }
}

/// Every armed signal stream the settled graph names, with the service its CURRENT generation
/// publishes on.
fn armed(g: &Graph) -> HashMap<(Uid, String), (String, StreamId)> {
    let mut out = HashMap::new();
    for uid in g.all_uids() {
        for output in g.recorded(uid).unwrap_or(&[]) {
            let slot = &output.slot;
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


/// One frame onto the writer, read for what only this thread can read. The file it belongs in, and
/// every byte of formatting, is the writer's: formatting here made the drain slower than a fast
/// producer, and a drain that falls behind its transport loses another stream's frames.
fn hand(recorder: &Recorder, time: &Time, feed: &Feed, bytes: &[u8], finally: bool, drift: &mut Option<f64>) -> bool {
    let meta = goofi_codec::frame_meta(bytes).ok();
    let at = meta.as_ref().and_then(|m| m.time()).unwrap_or_else(|| time.now());
    let rate = feed.rate.then(|| meta.as_ref().and_then(|m| m.sfreq()).unwrap_or_default());
    if let Some(goofi_core::MetaValue::Float(d)) = meta.as_ref().and_then(|m| m.get(goofi_core::META_DRIFT)) {
        *drift = Some(*d);
    }
    recorder.take_frame(&feed.id, bytes, rate, feed.timeline, at, finally)
}

/// Take every frame the feed holds, in one pass, and write each straight through. A recorder that
/// kept only the newest frame is the defect this feature exists to prevent, so nothing stops at one.
///
/// Nothing here counts a loss and nothing here MAKES one: a frame the lane refuses is HELD rather
/// than thrown away, because it has already left the transport and no numbering could show it
/// gone. What overflows is then the subscriber's oldest, which the next frame's own number says.
fn drain_feed(recorder: &Recorder, time: &Time, feed: &mut Feed, finally: bool) {
    let mut taken = 0usize;
    let mut drift: Option<f64> = None;
    if let Some(held) = feed.held.take() {
        if !hand(recorder, time, feed, &held, finally, &mut drift) {
            feed.held = Some(held);
            return;
        }
        taken += 1;
    }
    while let Ok(Some(sample)) = feed.subscriber.receive() {
        let bytes = sample.payload();
        if !hand(recorder, time, feed, bytes, finally, &mut drift) {
            feed.held = Some(bytes.to_vec());
            break;
        }
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
    if taken > 0 {
        recorder.note();
    }
}

impl Drain {
    /// Reconcile the held feeds against the settled graph. A departing feed is drained TO
    /// EXHAUSTION before it is closed: what the service already delivered is transported data, and
    /// dropping the subscriber would lose it where no gap and no count could ever show it.
    fn resolve(&mut self) -> bool {
        let wanted = armed(&self.graph.lock().unwrap_or_else(|e| e.into_inner()));
        let expected = wanted.len();
        for key in self.feeds.keys().cloned().collect::<Vec<_>>() {
            let why = match wanted.get(&key) {
                Some((service, _)) if *service == self.feeds[&key].service => continue,
                Some(_) => "reborn",
                None => "disarmed",
            };
            let mut feed = self.feeds.remove(&key).expect("a key just read");
            drain_feed(&self.recorder, &self.time, &mut feed, true);
            self.recorder.close(&feed.id, why);
        }
        for (key, (service, id)) in wanted {
            if self.feeds.contains_key(&key) {
                continue;
            }
            let shape = record_shape(id.engine);
            let timeline = timeline(id.engine).expect("armed filtered the engines above");
            if let Ok(subscriber) = goofi_transport::open_record_subscriber(&self.node, &service, shape) {
                let feed = Feed {
                    rate: id.engine == "audio",
                    id,
                    service,
                    subscriber,
                    held: None,
                    timeline,
                    buffer: shape.buffer,
                };
                self.feeds.insert(key, feed);
            }
        }
        self.feeds.len() == expected
    }

    fn sweep(&mut self, finally: bool) {
        for feed in self.feeds.values_mut() {
            drain_feed(&self.recorder, &self.time, feed, finally);
        }
    }

    /// Every feed drained to exhaustion and let go. A stop asks for this before it closes a file,
    /// so the frames the last sweep did not reach are not the tail this recording loses.
    fn release(&mut self) {
        for (_, mut feed) in self.feeds.drain().collect::<Vec<_>>() {
            drain_feed(&self.recorder, &self.time, &mut feed, true);
        }
    }
}

fn drain_epoch(graph: &Arc<Mutex<Graph>>) -> Arc<std::sync::atomic::AtomicU64> {
    graph.lock().unwrap_or_else(|e| e.into_inner()).epoch()
}

/// Start the one drain. `halt` is what stops it, and what a teardown waits on to a ceiling.
pub fn spawn(graph: Arc<Mutex<Graph>>, recorder: Arc<Recorder>, halt: Arc<Halt>) {
    recorder.set_capture(Arc::new(AudioCapture(Arc::downgrade(&graph))));
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
        let epoch = drain_epoch(&graph);
        let mut drain = Drain { graph, recorder, time, feeds: HashMap::new(), listener, node };
        // The graph epoch the feeds were resolved against; `None` once they were let go. The lock
        // is taken to resolve ONLY when that epoch moved — never per wake, which is the publish rate.
        let mut resolved: Option<u64> = None;
        let mut swept = 0;
        let mut ready = false;
        while !halt.stopped() {
            // The event id is ignored, which spends none of the id budget: a burst across every
            // armed slot coalesces into one sweep.
            let _ = drain.listener.timed_wait_all(|_| {}, WAKE);
            // Read BEFORE the sweep: a sweep already under way when a stop asked is not an answer
            // to it.
            let mark = drain.recorder.sweeping();
            if !drain.recorder.receiving() {
                drain.release();
                drain.recorder.swept(mark);
                swept = mark;
                resolved = None;
                continue;
            }
            let now = epoch.load(std::sync::atomic::Ordering::Acquire);
            if resolved != Some(now) || mark != swept {
                ready = drain.resolve();
                resolved = Some(now);
            }
            // Keep subscribers ready during preparation. A requested sweep drains with
            // backpressure so its acknowledgement includes every queued frame.
            drain.sweep(mark != swept);
            if ready {
                drain.recorder.swept(mark);
                swept = mark;
            }
        }
        drain.release();
        drop(drain);
        halt.release();
    });
    if started.is_err() {
        // The thread is what releases the halt, so a teardown must not wait on one that never ran.
        ran.release();
    }
}
