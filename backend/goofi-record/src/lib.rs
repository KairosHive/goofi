//! The recorder: a folder, one manifest, and one writer per stream. It owns no engine and no
//! engine owns it.

pub mod manifest;
pub mod stream;

use goofi_core::time::{stamp, stamp_nanos, Time};
use goofi_node::Uid;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

pub use manifest::Manifest;
pub use stream::{Kind, Stream, StreamMeta, Timeline};

/// A poisoned lock is a panicked writer, and a recording that keeps writing beats a panic in
/// every other drain.
fn held<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

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
        let (size, fps) = match s.kind {
            Kind::Frames => (None, None),
            Kind::Video { size, fps } => (Some(size), Some(fps)),
        };
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
            channels: s.meta.channels,
            size,
            fps,
            frames: s.frames,
            dropped: s.dropped,
            dropped_at: s.dropped_at,
            closed_because: because,
            error,
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

    fn close(&mut self, id: &StreamId, why: &str) {
        if let Some(s) = self.open.remove(id) {
            let mut s = held(&s);
            let error = s.sync().err();
            self.closed.push(Session::entry(id, &s, Some(why.to_string()), error));
        }
    }
}

pub struct Recorder {
    time: Arc<Time>,
    session: Mutex<Option<Session>>,
}

impl Recorder {
    pub fn new(time: Arc<Time>) -> Recorder {
        Recorder { time, session: Mutex::new(None) }
    }

    fn held(&self) -> MutexGuard<'_, Option<Session>> {
        held(&self.session)
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
        let Some(mut session) = self.held().take() else { return Ok(None) };
        let ids: Vec<StreamId> = session.open.keys().cloned().collect();
        for id in &ids {
            session.close(id, "stopped");
        }
        session.stopped_utc = Some(self.time.utc());
        let folder = session.folder.clone();
        session.manifest(&self.time).write_atomic(&folder)?;
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
        let file = session.free_name(&base, kind.extension());
        let stream = Stream::create(&session.folder, file, kind, meta, t0_patch, t0_utc)?;
        session.open.insert(id.clone(), Arc::new(Mutex::new(stream)));
        let folder = session.folder.clone();
        session.manifest(&self.time).write_atomic(&folder)
    }

    pub fn close(&self, id: &StreamId, why: &str) {
        let mut guard = self.held();
        let Some(session) = guard.as_mut() else { return };
        session.close(id, why);
        let folder = session.folder.clone();
        let _ = session.manifest(&self.time).write_atomic(&folder);
    }

    /// Write one already-encoded frame. The session is unlocked before the disk is touched, so a
    /// slow stream never stalls another engine's drain.
    pub fn write(&self, id: &StreamId, frame: &[u8]) -> Result<(), String> {
        let stream = {
            let guard = self.held();
            let session = guard.as_ref().ok_or("no recording is running")?;
            session.open.get(id).ok_or("no such open stream")?.clone()
        };
        let mut stream = held(&stream);
        stream.write(frame)
    }

    pub fn dropped(&self, id: &StreamId, count: u64, at: f64) {
        let mut guard = self.held();
        let Some(session) = guard.as_mut() else { return };
        let Some(stream) = session.open.get(id) else { return };
        {
            let mut stream = held(stream);
            stream.dropped += count;
            stream.dropped_at = Some(at);
        }
        let folder = session.folder.clone();
        let _ = session.manifest(&self.time).write_atomic(&folder);
    }

    /// How full the stream's feeding buffer is, as its drain last saw it.
    pub fn fill(&self, id: &StreamId, fill: f32) {
        let guard = self.held();
        if let Some(stream) = guard.as_ref().and_then(|s| s.open.get(id)) {
            held(stream).fill = fill;
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
                        frames: s.frames,
                        dropped: s.dropped,
                        fill: s.fill,
                    }
                })
                .collect(),
        }
    }
}
