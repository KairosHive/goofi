//! The recorder: a folder, one manifest, and one writer per stream. It owns no engine and no
//! engine owns it.

pub mod manifest;
pub mod stream;
pub mod video;

use goofi_core::time::{stamp, stamp_nanos, Time};
use goofi_node::Uid;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime};

pub use manifest::Manifest;
pub use stream::{Kind, Stream, StreamMeta, Timeline};

/// A poisoned lock is a panicked writer, and a recording that keeps writing beats a panic in
/// every other drain.
fn held<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The longest a stop waits for its drains and reapers.
const SETTLE: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamId {
    pub uid: Uid,
    pub node: String,
    pub slot: String,
    pub engine: &'static str,
}

pub struct Status {
    pub running: bool,
    pub folder: Option<PathBuf>,
    pub started: Option<f64>,
    pub streams: Vec<StreamStatus>,
}

pub struct StreamStatus {
    pub node: String,
    pub slot: String,
    pub engine: &'static str,
    pub file: String,
    pub frames: u64,
    pub dropped: u64,
    pub fill: f32,
}

/// Finalize a detached stream — the wait for its encoder — and the entry it leaves behind.
fn finished(id: &StreamId, stream: &Arc<Mutex<Stream>>, why: &str) -> manifest::Entry {
    let mut s = held(stream);
    let error = s.sync().err();
    Session::entry(id, &s, Some(why.to_string()), error)
}

/// What a stream's kind puts in its manifest entry — and, for a video, what its encoding costs.
fn shape_of(kind: Kind) -> (Option<(u32, u32)>, Option<f64>, Option<&'static str>) {
    match kind {
        Kind::Frames => (None, None, None),
        Kind::Video { size, fps } => (Some(size), Some(fps), Some(video::CLIP)),
    }
}

struct Session {
    folder: PathBuf,
    name: String,
    patch: Option<PathBuf>,
    started: f64,
    stopped_utc: Option<SystemTime>,
    open: BTreeMap<StreamId, Arc<Mutex<Stream>>>,
    closed: Vec<manifest::Entry>,
}

impl Session {
    fn entry(
        id: &StreamId,
        s: &Stream,
        because: Option<String>,
        error: Option<String>,
    ) -> manifest::Entry {
        let (size, fps, encoding) = shape_of(s.kind);
        manifest::Entry {
            file: s.file.clone(),
            node: id.node.clone(),
            slot: id.slot.clone(),
            uid: id.uid.to_hex(),
            engine: id.engine,
            t0_utc: stamp_nanos(s.t0_utc),
            t0_patch: s.t0_patch,
            sfreq: s.meta.sfreq,
            timeline: s.meta.timeline.name(),
            drift: s.drift,
            channels: s.meta.channels,
            size,
            fps,
            encoding,
            frames: s.frames(),
            dropped: s.lost(),
            dropped_at: s.dropped_at,
            closed_because: because,
            error,
        }
    }

    /// The entry a stream that never opened leaves behind.
    fn missing(
        id: &StreamId,
        file: &str,
        kind: Kind,
        meta: &StreamMeta,
        t0_patch: f64,
        t0_utc: SystemTime,
        why: &str,
    ) -> manifest::Entry {
        let (size, fps, encoding) = shape_of(kind);
        manifest::Entry {
            file: file.to_string(),
            node: id.node.clone(),
            slot: id.slot.clone(),
            uid: id.uid.to_hex(),
            engine: id.engine,
            t0_utc: stamp_nanos(t0_utc),
            t0_patch,
            sfreq: meta.sfreq,
            timeline: meta.timeline.name(),
            drift: None,
            channels: meta.channels,
            size,
            fps,
            encoding,
            frames: 0,
            dropped: 0,
            dropped_at: None,
            closed_because: Some("never opened".to_string()),
            error: Some(why.to_string()),
        }
    }

    fn manifest(&self, time: &Time) -> Manifest {
        let mut streams = self.closed.clone();
        streams.extend(self.open.iter().map(|(id, s)| Session::entry(id, &held(s), None, None)));
        Manifest {
            goofi: env!("CARGO_PKG_VERSION"),
            name: self.name.clone(),
            patch: self.patch.as_ref().map(|p| p.display().to_string()),
            origin_utc: stamp_nanos(time.wall()),
            started_utc: stamp(time.utc_at(self.started)),
            stopped_utc: self.stopped_utc.map(stamp),
            streams,
        }
    }

    /// The name nothing else in the folder holds, so a file is never opened over a written one.
    fn free_name(&self, base: &str, ext: &str) -> String {
        (1u32..)
            .map(|n| if n == 1 { format!("{base}.{ext}") } else { format!("{base}-{n}.{ext}") })
            .find(|file| !self.folder.join(file).exists())
            .expect("a free name, of endlessly many")
    }

    /// Take a stream out. Nothing else can reach it once it is out of the map, which is what
    /// lets the caller finalize it with no lock held.
    fn detach(&mut self, id: &StreamId) -> Option<Arc<Mutex<Stream>>> {
        self.open.remove(id)
    }

    /// Close a stream this session OWNS outright — the caller holds no lock anyone else needs.
    fn close(&mut self, id: &StreamId, why: &str) {
        if let Some(s) = self.detach(id) {
            self.closed.push(finished(id, &s, why));
        }
    }
}

pub struct Recorder {
    time: Arc<Time>,
    session: Mutex<Option<Session>>,
    encoders: Mutex<Arc<dyn video::Encoders>>,
    /// Every manifest snapshot is numbered under the session lock and lands in that order: a
    /// slow write is SKIPPED where a newer one already reached the disk. Nothing holds the
    /// session while writing, so a slow disk cannot park a stream's own frames.
    version: AtomicU64,
    landed: Mutex<u64>,
    /// The sweeps a stop has asked its drains for, and the ones they have finished. A stop waits
    /// for the second to reach the first, so no file is closed over a frame already delivered.
    asked: AtomicU64,
    swept: AtomicU64,
}

impl Recorder {
    pub fn new(time: Arc<Time>) -> Recorder {
        Recorder {
            time,
            session: Mutex::new(None),
            encoders: Mutex::new(Arc::new(video::FfmpegEncoders)),
            version: AtomicU64::new(0),
            landed: Mutex::new(0),
            asked: AtomicU64::new(0),
            swept: AtomicU64::new(0),
        }
    }

    /// The encoder every video stream opens through. One swap changes the codec, and nothing
    /// above this knows there was one.
    pub fn set_encoders(&self, encoders: Arc<dyn video::Encoders>) {
        *held(&self.encoders) = encoders;
    }

    /// Whether a video stream could be opened at all. `record start` asks it, and refuses only a
    /// recording that would hold nothing else.
    pub fn can_encode(&self) -> Result<(), String> {
        let encoders = held(&self.encoders).clone();
        encoders.probe()
    }

    fn held(&self) -> MutexGuard<'_, Option<Session>> {
        held(&self.session)
    }

    /// The manifest as the session stands, written with the session UNLOCKED — a disk that is
    /// slow must never park the engine writing frames through it. The snapshot is numbered under
    /// the session lock, and [`Recorder::land`] is what keeps the writes in that order.
    fn publish(&self, guard: MutexGuard<'_, Option<Session>>) -> Result<(), String> {
        let Some(session) = guard.as_ref() else { return Ok(()) };
        let manifest = session.manifest(&self.time);
        let folder = session.folder.clone();
        let version = self.version.fetch_add(1, Ordering::SeqCst) + 1;
        drop(guard);
        self.land(&manifest, &folder, version)
    }

    /// Write one numbered snapshot, SKIPPING it where a newer one already reached the disk. Every
    /// manifest write goes through here, so no path can land out of order.
    fn land(&self, manifest: &Manifest, folder: &Path, version: u64) -> Result<(), String> {
        let mut landed = held(&self.landed);
        if *landed > version {
            return Ok(());
        }
        let done = manifest.write_atomic(folder);
        *landed = version;
        done
    }

    /// A sweep a stop is waiting for. The drain reads this before it sweeps and reports it after,
    /// so a sweep that began before the ask never counts as the answer to it.
    pub fn sweeping(&self) -> u64 {
        self.asked.load(Ordering::SeqCst)
    }

    /// One sweep of every feed, to exhaustion, has finished.
    pub fn swept(&self, mark: u64) {
        self.swept.fetch_max(mark, Ordering::SeqCst);
    }

    /// Ask every drain for one more sweep and wait for it, to a CEILING — a wedged drain must
    /// never wedge a stop. What it buys is the tail: the frames the last sweep did not reach.
    fn settle(&self) {
        let want = self.asked.fetch_add(1, Ordering::SeqCst) + 1;
        let deadline = Instant::now() + SETTLE;
        while self.swept.load(Ordering::SeqCst) < want && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Mint the folder. Refused when a recording already runs — the session lock is the ONE
    /// authority on that, so no caller can check and then act past it.
    pub fn start(&self, root: &Path, name: &str, patch: Option<&Path>) -> Result<PathBuf, String> {
        let mut held = self.held();
        if held.is_some() {
            return Err("a recording already runs".into());
        }
        let started = self.time.now();
        let stamped = format!("{}Z", stamp(self.time.utc_at(started)));
        let folder = root.join(if name.is_empty() { stamped } else { format!("{stamped}-{name}") });
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let session = Session {
            folder: folder.clone(),
            name: name.to_string(),
            patch: patch.map(Path::to_path_buf),
            started,
            stopped_utc: None,
            open: BTreeMap::new(),
            closed: Vec::new(),
        };
        session.manifest(&self.time).write_atomic(&folder)?;
        *held = Some(session);
        Ok(folder)
    }

    /// Close every stream and finalize the manifest. `Ok(None)` is a recorder that was not
    /// running; a manifest that could not be written is the error, never a silent `None`.
    pub fn stop(&self) -> Result<Option<PathBuf>, String> {
        if !self.running() {
            return Ok(None);
        }
        self.settle();
        let mut guard = self.held();
        let Some(mut session) = guard.take() else { return Ok(None) };
        // Numbered while the session is still held: it outranks every snapshot taken before the
        // take, after which nothing holds a session to mint another from.
        let version = self.version.fetch_add(1, Ordering::SeqCst) + 1;
        drop(guard);
        let ids: Vec<StreamId> = session.open.keys().cloned().collect();
        for id in &ids {
            session.close(id, "stopped");
        }
        session.stopped_utc = Some(self.time.utc());
        let folder = session.folder.clone();
        self.land(&session.manifest(&self.time), &folder, version)?;
        Ok(Some(folder))
    }

    pub fn running(&self) -> bool {
        self.held().is_some()
    }

    /// Open a file for a stream, closing whatever that stream held.
    pub fn open(
        &self,
        id: &StreamId,
        kind: Kind,
        t0_patch: f64,
        meta: StreamMeta,
    ) -> Result<(), String> {
        let mut guard = self.held();
        let session = guard.as_mut().ok_or("no recording is running")?;
        session.close(id, "reopened");
        let t0_utc = self.time.utc_at(t0_patch);
        let base = format!("{}-{}__{}Z", id.node, id.slot, stamp_nanos(t0_utc));
        let encoders = held(&self.encoders).clone();
        let file = session.free_name(&base, kind.extension(&*encoders));
        let made = Stream::create(&*encoders, &session.folder, file.clone(), kind, meta.clone(), t0_patch, t0_utc);
        // A stream that could not open is an ENTRY, not an absence: a recording says what it was
        // asked for and did not get, or nobody reading it later can tell.
        let opened = match made {
            Ok(stream) => {
                session.open.insert(id.clone(), Arc::new(Mutex::new(stream)));
                Ok(())
            }
            Err(why) => {
                session.closed.push(Session::missing(id, &file, kind, &meta, t0_patch, t0_utc, &why));
                Err(why)
            }
        };
        // The manifest is a projection and is rewritten at every later publish, so a stream that
        // opened is OPEN even where the disk refused this snapshot.
        let _ = self.publish(guard);
        opened
    }

    /// Count what a stream lost and close it, on a thread of its own. Finalizing a video waits
    /// for its encoder, and a thread that renders must never wait for one.
    pub fn close_later(self: &Arc<Self>, id: &StreamId, why: &str, missed: u64, at: f64) {
        let (rec, mine, said) = (self.clone(), id.clone(), why.to_string());
        let closing = std::thread::Builder::new().name("goofi-record-close".into()).spawn(move || {
            if missed > 0 {
                rec.dropped(&mine, missed, at);
            }
            rec.close(&mine, &said);
        });
        // No thread to spare is no reason to leak the encoder: close it here instead.
        if closing.is_err() {
            if missed > 0 {
                self.dropped(id, missed, at);
            }
            self.close(id, why);
        }
    }

    /// Close a stream and finalize its file. A VIDEO's encoder is waited for here, so this is
    /// never called from a thread that renders — [`Recorder::close_later`] is that door.
    pub fn close(&self, id: &StreamId, why: &str) {
        let detached = self.held().as_mut().and_then(|s| s.detach(id));
        let Some(stream) = detached else { return };
        // NO lock is held here: finalizing a video waits for its encoder, and every other stream
        // and every other op must go through while it does.
        let entry = finished(id, &stream, why);
        let mut guard = self.held();
        if let Some(session) = guard.as_mut() {
            session.closed.push(entry);
        }
        let _ = self.publish(guard);
    }

    /// Write one already-encoded frame, carrying the frames MISSING before it. The write and the
    /// count land under ONE stream lock, so a frame is in the file if and only if its own gap was
    /// counted — a stop cannot take the session between the two and leave a gap nothing accounts
    /// for.
    pub fn write(&self, id: &StreamId, frame: &[u8], missed: u64, at: f64) -> Result<(), String> {
        let stream = {
            let guard = self.held();
            let session = guard.as_ref().ok_or("no recording is running")?;
            session.open.get(id).ok_or("no such open stream")?.clone()
        };
        let mut stream = held(&stream);
        let done = stream.write(frame);
        if done.is_ok() && missed > 0 {
            stream.dropped += missed;
            stream.dropped_at = Some(at);
        }
        done
    }

    /// Rewrite the manifest for what a sweep changed. Once per sweep: a rewrite per frame would
    /// make a drain that has fallen behind fall further behind.
    pub fn note(&self) {
        let _ = self.publish(self.held());
    }

    /// Hand one video readback to its encoder, dated by the tick that DREW it rather than the one
    /// that took it. `false` is a drop the caller counts: a real-time engine is never stalled.
    pub fn write_video(&self, id: &StreamId, texels: &[u8], at: f64) -> bool {
        let stream = {
            let guard = self.held();
            let Some(session) = guard.as_ref() else { return false };
            let Some(stream) = session.open.get(id) else { return false };
            stream.clone()
        };
        let mut stream = held(&stream);
        stream.write_video(texels, at)
    }

    pub fn dropped(&self, id: &StreamId, count: u64, at: f64) {
        let guard = self.held();
        let Some(session) = guard.as_ref() else { return };
        let Some(stream) = session.open.get(id) else { return };
        {
            let mut stream = held(stream);
            stream.dropped += count;
            stream.dropped_at = Some(at);
        }
        let _ = self.publish(guard);
    }

    /// What a derived timeline last measured itself against patch time. The manifest carries it so
    /// an analyst can correct the stream against the ones that read the clock.
    pub fn drift(&self, id: &StreamId, seconds: f64) {
        let guard = self.held();
        if let Some(stream) = guard.as_ref().and_then(|s| s.open.get(id)) {
            held(stream).drift = Some(seconds);
        }
    }

    /// Whether this stream has a file open right now — what a drain asks so it opens one exactly
    /// where the recorder holds none, rather than keeping a second belief about it.
    pub fn is_open(&self, id: &StreamId) -> bool {
        self.held().as_ref().is_some_and(|s| s.open.contains_key(id))
    }

    /// How full the stream's feeding buffer is, as its drain last saw it.
    pub fn fill(&self, id: &StreamId, fill: f32) {
        let guard = self.held();
        if let Some(stream) = guard.as_ref().and_then(|s| s.open.get(id)) {
            held(stream).set_fill(fill);
        }
    }

    pub fn status(&self) -> Status {
        let guard = self.held();
        let Some(session) = guard.as_ref() else {
            return Status { running: false, folder: None, started: None, streams: Vec::new() };
        };
        Status {
            running: true,
            folder: Some(session.folder.clone()),
            started: Some(session.started),
            streams: session
                .open
                .iter()
                .map(|(id, s)| {
                    let s = held(s);
                    StreamStatus {
                        node: id.node.clone(),
                        slot: id.slot.clone(),
                        engine: id.engine,
                        file: s.file.clone(),
                        frames: s.frames(),
                        dropped: s.lost(),
                        fill: s.fill(),
                    }
                })
                .collect(),
        }
    }
}
