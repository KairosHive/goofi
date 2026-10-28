//! The recorder: every armed output's frames, through the transport's queues, into one folder of
//! files under one manifest. It owns no engine and no engine owns it: an engine publishes frames,
//! and a worker here, one per armed stream, takes them off the queue and writes them on its own
//! time. Nothing a worker does reaches an engine's thread.

pub mod beside;
pub mod csv;
pub mod manifest;
pub mod npy;
pub mod stream;
pub mod video;
pub mod wav;
mod worker;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use goofi_core::record::VideoQuality;
use goofi_supervisor::timing::{stamp, stamp_nanos, Time};
use goofi_node::Uid;
use goofi_supervisor::sync::{Condvar, Mutex};
use goofi_supervisor::timing::Event;
use goofi_transport::Hub;

pub use manifest::Manifest;
pub use stream::{Kind, Stream, Timeline};
pub use worker::Live;

/// What a start and a stop ask of the engines: every control half delivers what it holds, so a
/// frame rendered before the boundary is on its queue before the file closes; and the audio
/// clock's block count, which bounds the derived timeline where no instant can.
pub trait Engines: Send + Sync {
    fn flush(&self) -> Result<(), String>;
    fn audio_blocks(&self) -> Option<u64>;
}

/// How often a worker's counts reach the manifest. The manifest is a PROJECTION, and a rewrite is
/// a create, an fsync and a rename: once a second is news; once a frame would be the cost.
const NOTE_EVERY: Duration = Duration::from_secs(1);

/// A ceiling on the wait for departed workers at shutdown, never on a stop: a stop waits for its
/// files, however long an encoder takes to finish one.
const LEAVE_WAIT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamId {
    pub uid: Uid,
    pub node: String,
    pub slot: String,
    pub engine: &'static str,
}

/// One armed output, as the settled graph names it: the queue its frames ride, how deep, how its
/// frames are dated, and the quality a video of it is encoded at.
#[derive(Clone, Debug, PartialEq)]
pub struct Armed {
    pub id: StreamId,
    pub service: String,
    pub depth: usize,
    pub timeline: Timeline,
    pub quality: VideoQuality,
}

pub struct Status {
    pub running: bool,
    pub folder: Option<PathBuf>,
    pub started: Option<f64>,
    pub streams: Vec<StreamStatus>,
}

#[derive(serde::Serialize)]
pub struct StreamStatus {
    pub node: String,
    pub slot: String,
    pub engine: &'static str,
    pub file: String,
    pub frames: u64,
    pub dropped: u64,
    pub fill: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Where a take begins or ends: the instant, for a stream that reads the clock, and the audio
/// clock's block, for one that counts samples.
#[derive(Clone, Copy, Debug)]
pub struct Boundary {
    pub time: f64,
    pub block: u64,
}

/// One recording: its folder, its boundaries, and the entries of the streams that closed in it.
pub struct Take {
    /// Which take this is, so a worker tells the one it opened a file in from the next.
    pub id: u64,
    pub folder: PathBuf,
    name: String,
    patch: Option<PathBuf>,
    pub started: f64,
    pub begin: Boundary,
    /// Set by the stop, before the engines are flushed: a frame past it is the next take's.
    pub end: Mutex<Option<Boundary>>,
    /// Raised once the engines have delivered what they held past the end: every frame of this
    /// take is on its queue, and a worker whose queue is empty is done.
    flushed: AtomicBool,
    stopped_utc: Mutex<Option<SystemTime>>,
    pub closed: Mutex<Vec<manifest::Entry>>,
    /// The manifest versions minted and landed, so a slower writer never lands an older one.
    version: AtomicU64,
    landed: Mutex<u64>,
}

impl Take {
    /// Whether the stop has delivered every frame of this take to the queues.
    pub fn stopping(&self) -> bool {
        self.flushed.load(Ordering::Acquire)
    }

    /// Whether a frame dated `at`, numbered `index`, falls inside this take.
    pub fn holds(&self, timeline: Timeline, at: f64, index: Option<u64>) -> bool {
        let end = *self.end.lock();
        match timeline {
            Timeline::Measured => at >= self.begin.time && end.is_none_or(|e| at < e.time),
            Timeline::Derived => index.is_some_and(|i| i >= self.begin.block && end.is_none_or(|e| i < e.block)),
        }
    }

    /// The name nothing else in the folder holds, so a file is never opened over a written one.
    pub fn free_name(&self, base: &str, ext: &str) -> String {
        (1u32..)
            .map(|n| if n == 1 { format!("{base}.{ext}") } else { format!("{base}-{n}.{ext}") })
            .find(|file| !self.folder.join(file).exists())
            .expect("a free name, of endlessly many")
    }
}

pub struct Recorder {
    time: Arc<Time>,
    hub: Arc<Hub>,
    take: Mutex<Option<Arc<Take>>>,
    takes: AtomicU64,
    /// One worker per armed stream, by its id.
    workers: Mutex<BTreeMap<StreamId, worker::Handle>>,
    /// Workers told to leave, each writing what its queue held and finishing its file on its own
    /// time: listed, woken and projected like the armed ones until they are gone.
    departing: Mutex<Vec<worker::Handle>>,
    /// A stop parks here until every worker has closed its file in the take.
    done: Mutex<()>,
    finished: Condvar,
    /// Held: every worker parks instead of reading, so a test can fill a queue to overflow.
    held: AtomicBool,
    /// Whether a take runs, readable from any thread: what puts an armed stage in demand.
    running: Arc<AtomicBool>,
    encoders: Mutex<Arc<dyn video::Encoders>>,
    engines: Mutex<Option<Arc<dyn Engines>>>,
    noted: Mutex<Instant>,
    transition: Mutex<()>,
}

impl Recorder {
    pub fn new(time: Arc<Time>, hub: Arc<Hub>) -> Recorder {
        Recorder {
            time,
            hub,
            take: Mutex::new(None),
            takes: AtomicU64::new(0),
            workers: Mutex::new(BTreeMap::new()),
            departing: Mutex::new(Vec::new()),
            done: Mutex::new(()),
            finished: Condvar::new(),
            held: AtomicBool::new(false),
            running: Arc::new(AtomicBool::new(false)),
            encoders: Mutex::new(Arc::new(video::FfmpegEncoders)),
            engines: Mutex::new(None),
            noted: Mutex::new(Instant::now()),
            transition: Mutex::new(()),
        }
    }

    /// The encoder every video stream opens through. One swap changes the codec, and nothing
    /// above this knows there was one.
    pub fn set_encoders(&self, encoders: Arc<dyn video::Encoders>) {
        *self.encoders.lock() = encoders;
    }

    pub fn set_engines(&self, engines: Arc<dyn Engines>) {
        *self.engines.lock() = Some(engines);
    }

    /// Whether a video stream could be opened at all. `record start` asks it, and refuses only a
    /// recording that would hold nothing else.
    pub fn can_encode(&self) -> Result<(), String> {
        let encoders = self.encoders.lock().clone();
        encoders.probe()
    }

    /// Whether a take runs, as a flag an engine's clock reads without a lock.
    pub fn running_flag(&self) -> Arc<AtomicBool> {
        self.running.clone()
    }

    pub fn running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    /// The take that runs, if one does.
    pub(crate) fn current(&self) -> Option<Arc<Take>> {
        self.take.lock().clone()
    }

    pub(crate) fn encoders(&self) -> Arc<dyn video::Encoders> {
        self.encoders.lock().clone()
    }

    pub(crate) fn time(&self) -> &Time {
        &self.time
    }

    /// Park every worker, or let them go. A held recorder's queues fill and overflow at their
    /// depth, which is how a test witnesses the count of what overflowed.
    pub fn hold(&self, held: bool) {
        self.held.store(held, Ordering::Release);
        if !held {
            self.poke();
        }
    }

    pub(crate) fn held(&self) -> bool {
        self.held.load(Ordering::Acquire)
    }

    /// Wake every worker, so one that parks on an empty queue reads what moved.
    fn poke(&self) {
        for shared in self.handles() {
            shared.event.ring(goofi_transport::POKED);
        }
    }

    /// Every worker there is: the armed ones and the ones on their way out that are not yet gone.
    fn handles(&self) -> Vec<Arc<worker::Shared>> {
        let mut all: Vec<Arc<worker::Shared>> = self.workers.lock().values().map(|h| h.shared.clone()).collect();
        all.extend(self.departing.lock().iter().filter(|h| !h.gone()).map(|h| h.shared.clone()));
        all
    }

    /// A worker is done with take `id`, or gone for good.
    pub(crate) fn finished(&self, shared: &worker::Shared, id: u64) {
        shared.finished.store(id, Ordering::Release);
        let _done = self.done.lock();
        self.finished.notify_all();
    }

    /// The streams to record, whole: the settled graph's armed outputs. A stream named here and
    /// not before gets a worker on its queue; one no longer named has its worker close its file and
    /// leave, after what the queue already holds; one named under a new queue is reborn.
    pub fn arm(self: &Arc<Self>, armed: Vec<Armed>) {
        let mut workers = self.workers.lock();
        let mut departing = self.departing.lock();
        departing.retain(|h| !h.gone());
        for (id, handle) in std::mem::take(&mut *workers) {
            let wanted = armed.iter().find(|a| a.id == id);
            match wanted {
                Some(a) if a.service == handle.armed.lock().service => {
                    handle.armed.lock().quality = a.quality;
                    workers.insert(id, handle);
                }
                Some(_) => departing.push(handle.leave("reborn")),
                None => departing.push(handle.leave("disarmed")),
            }
        }
        for a in armed {
            if let std::collections::btree_map::Entry::Vacant(slot) = workers.entry(a.id.clone()) {
                if let Some(handle) = worker::spawn(self, a) {
                    slot.insert(handle);
                }
            }
        }
    }

    /// Mint the folder and begin the take. Refused when one already runs — the take lock is the
    /// ONE authority on that, so no caller can check and then act past it.
    pub fn start(self: &Arc<Self>, root: &Path, name: &str, patch: Option<&Path>, annotations: Option<&serde_json::Value>) -> Result<PathBuf, String> {
        let _transition = self.transition.lock();
        if self.running() {
            return Err("a recording already runs".into());
        }
        if name.chars().any(|c| c.is_control() || "/\\:<>\"|?*".contains(c)) {
            return Err("recording name contains a path separator or unsupported filename character".into());
        }
        let annotations = annotations.map(|v| v.as_object().ok_or("annotations must be an object")).transpose()?;
        if let Some(annotations) = annotations {
            for (namespace, value) in annotations {
                if namespace.is_empty() || namespace.len() > 80 || !namespace.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
                    return Err("annotation namespace must contain lowercase letters, digits, or hyphens".into());
                }
                if serde_json::to_vec(value).map_err(|e| e.to_string())?.len() > 1024 * 1024 {
                    return Err("annotation exceeds 1 MiB".into());
                }
            }
        }
        let started = self.time.now();
        let stamped = format!("{}Z", stamp(self.time.utc_at(started)));
        let folder = root.join(if name.is_empty() { stamped } else { format!("{stamped}-{name}") });
        std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
        std::fs::create_dir(&folder).map_err(|e| e.to_string())?;
        if let Some(annotations) = annotations {
            for (namespace, value) in annotations {
                let write = || -> Result<(), String> {
                    use std::io::Write;
                    let path = folder.join("annotations").join(namespace);
                    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
                    let mut file = std::fs::File::create(path.join("session.json")).map_err(|e| e.to_string())?;
                    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
                    file.write_all(&bytes).and_then(|()| file.sync_all()).map_err(|e| e.to_string())
                };
                if let Err(error) = write() {
                    let _ = std::fs::remove_dir_all(&folder);
                    return Err(error);
                }
            }
        }
        let engines = self.engines.lock().clone();
        let block = engines.as_ref().and_then(|e| e.audio_blocks()).unwrap_or(0);
        let take = Arc::new(Take {
            id: self.takes.fetch_add(1, Ordering::Relaxed) + 1,
            folder: folder.clone(),
            name: name.to_string(),
            patch: patch.map(Path::to_path_buf),
            started,
            begin: Boundary { time: started, block },
            end: Mutex::new(None),
            flushed: AtomicBool::new(false),
            stopped_utc: Mutex::new(None),
            closed: Mutex::new(Vec::new()),
            version: AtomicU64::new(0),
            landed: Mutex::new(0),
        });
        if let Err(error) = self.manifest(&take).write_atomic(&folder) {
            let _ = std::fs::remove_dir_all(&folder);
            return Err(error);
        }
        *self.take.lock() = Some(take);
        self.running.store(true, Ordering::Release);
        self.poke();
        Ok(folder)
    }

    /// End the take: the boundary is set, the engines deliver what they hold, every worker writes
    /// what falls inside and closes its file, and the manifest is finalized. `Ok(None)` is a
    /// recorder that was not running; a manifest that could not be written is the error.
    pub fn stop(&self) -> Result<Option<PathBuf>, String> {
        let _transition = self.transition.lock();
        let Some(take) = self.current() else { return Ok(None) };
        let engines = self.engines.lock().clone();
        let block = engines.as_ref().and_then(|e| e.audio_blocks()).unwrap_or(u64::MAX);
        *take.end.lock() = Some(Boundary { time: self.time.now(), block });
        *take.stopped_utc.lock() = Some(self.time.utc());
        if let Some(engines) = engines {
            engines.flush()?;
        }
        take.flushed.store(true, Ordering::Release);
        // Every worker armed NOW is owed a wait; one that leaves meanwhile reports itself gone.
        let waited: Vec<Arc<worker::Shared>> = self.workers.lock().values().map(|h| h.shared.clone()).collect();
        self.poke();
        let mut done = self.done.lock();
        done = self.finished.wait_while(done, |_| waited.iter().any(|w| w.finished.load(Ordering::Acquire) < take.id));
        drop(done);
        *self.take.lock() = None;
        self.running.store(false, Ordering::Release);
        self.publish(&take)?;
        Ok(Some(take.folder.clone()))
    }

    /// Stop, send every worker away and wait for them: the manager's last word to the recorder.
    pub fn shutdown(self: &Arc<Self>) {
        if let Err(error) = self.stop() {
            goofi_supervisor::log::record(goofi_supervisor::log::Source::component("record"), goofi_supervisor::log::Level::Error, None, format!("Recording could not be finalized: {error}"));
        }
        self.arm(Vec::new());
        let deadline = goofi_supervisor::timing::Deadline::within(LEAVE_WAIT);
        for handle in std::mem::take(&mut *self.departing.lock()) {
            handle.join_within(deadline.left());
        }
    }

    fn manifest(&self, take: &Take) -> Manifest {
        let mut streams = take.closed.lock().clone();
        streams.extend(self.handles().iter().filter_map(|shared| {
            let live = shared.live.lock();
            live.entry.clone().filter(|_| live.take == take.id)
        }));
        Manifest {
            goofi: env!("CARGO_PKG_VERSION"),
            name: take.name.clone(),
            patch: take.patch.as_ref().map(|p| p.display().to_string()),
            origin_utc: stamp_nanos(self.time.wall()),
            started_utc: stamp(self.time.utc_at(take.started)),
            stopped_utc: take.stopped_utc.lock().map(stamp),
            streams,
        }
    }

    /// Rewrite the take's manifest: a projection of every entry closed and every file open, minted
    /// now and landed only if nothing newer landed first.
    pub(crate) fn publish(&self, take: &Take) -> Result<(), String> {
        let manifest = self.manifest(take);
        let version = take.version.fetch_add(1, Ordering::Relaxed) + 1;
        let mut landed = take.landed.lock();
        if *landed > version {
            return Ok(());
        }
        manifest.write_atomic(&take.folder)?;
        *landed = version;
        Ok(())
    }

    /// Publish for what the writes since the last note changed, on [`NOTE_EVERY`]. Every count is
    /// on its worker already, so a skipped note loses nothing: the next, and the stop, mint whole.
    pub(crate) fn note(&self, take: &Take) {
        {
            let mut noted = self.noted.lock();
            if noted.elapsed() < NOTE_EVERY {
                return;
            }
            *noted = Instant::now();
        }
        let _ = self.publish(take);
    }

    pub fn status(&self) -> Status {
        let take = self.current();
        let workers = self.workers.lock();
        let departing = self.departing.lock();
        let streams = workers
            .values()
            .chain(departing.iter().filter(|h| !h.gone()))
            .filter_map(|handle| {
                let id = &handle.armed.lock().id;
                let live = handle.shared.live.lock();
                let entry = live.entry.as_ref();
                if entry.is_none() && live.error.is_none() {
                    return None;
                }
                Some(StreamStatus {
                    node: id.node.clone(),
                    slot: id.slot.clone(),
                    engine: id.engine,
                    file: entry.map(|e| e.file.clone()).unwrap_or_default(),
                    frames: entry.map_or(0, |e| e.frames),
                    dropped: entry.map_or(0, |e| e.dropped),
                    fill: live.fill,
                    error: live.error.clone(),
                })
            })
            .collect();
        Status {
            running: take.is_some(),
            folder: take.as_ref().map(|t| t.folder.clone()),
            started: take.as_ref().map(|t| t.started),
            streams,
        }
    }
}

/// The event a worker parks on, held by the recorder to wake it: the queue's own.
pub(crate) type Wake = Arc<Event>;
