//! The recorder: a folder, one manifest, and one writer per stream. It owns no engine and no
//! engine owns it.

pub mod manifest;
pub mod stream;

use goofi_core::time::{stamp, stamp_nanos, Time};
use goofi_node::Uid;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

pub use manifest::Manifest;
pub use stream::{Kind, Stream, StreamMeta, Timeline};

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
    started_utc: SystemTime,
    stopped_utc: Option<SystemTime>,
    open: BTreeMap<StreamId, Stream>,
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

    fn manifest(&self, origin: SystemTime) -> Manifest {
        let mut streams = self.closed.clone();
        streams.extend(self.open.iter().map(|(id, s)| Session::entry(id, s, None, None)));
        Manifest {
            goofi: env!("CARGO_PKG_VERSION"),
            name: self.name.clone(),
            patch: self.patch.as_ref().map(|p| p.display().to_string()),
            origin_utc: stamp_nanos(origin),
            started_utc: stamp(self.started_utc),
            stopped_utc: self.stopped_utc.map(stamp),
            streams,
        }
    }

    fn close(&mut self, id: &StreamId, why: &str) {
        if let Some(mut s) = self.open.remove(id) {
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

    fn held(&self) -> std::sync::MutexGuard<'_, Option<Session>> {
        self.session.lock().expect("a poisoned recorder is a panicked writer")
    }

    /// Mint the folder. A recording already running is stopped first, so a start is total.
    pub fn start(&self, root: &Path, name: &str, patch: Option<&Path>) -> Result<PathBuf, String> {
        self.stop();
        let started_utc = self.time.utc();
        let stamped = format!("{}Z", stamp(started_utc));
        let folder = root.join(if name.is_empty() { stamped } else { format!("{stamped}-{name}") });
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let session = Session {
            folder: folder.clone(),
            name: name.to_string(),
            patch: patch.map(Path::to_path_buf),
            started: self.time.now(),
            started_utc,
            stopped_utc: None,
            open: BTreeMap::new(),
            closed: Vec::new(),
        };
        session.manifest(self.time.wall()).write_atomic(&folder)?;
        *self.held() = Some(session);
        Ok(folder)
    }

    /// Close every stream and finalize the manifest.
    pub fn stop(&self) -> Option<PathBuf> {
        let mut session = self.held().take()?;
        let ids: Vec<StreamId> = session.open.keys().cloned().collect();
        for id in &ids {
            session.close(id, "stopped");
        }
        session.stopped_utc = Some(self.time.utc());
        let folder = session.folder.clone();
        session.manifest(self.time.wall()).write_atomic(&folder).ok()?;
        Some(folder)
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
        let mut held = self.held();
        let session = held.as_mut().ok_or("no recording is running")?;
        session.close(id, "reopened");
        let t0_utc = self.time.utc_at(t0_patch);
        let file = format!(
            "{}-{}__{}Z.{}",
            id.node,
            id.slot,
            stamp_nanos(t0_utc),
            kind.extension()
        );
        let stream = Stream::create(&session.folder, file, kind, meta, t0_patch, t0_utc)?;
        session.open.insert(id.clone(), stream);
        let folder = session.folder.clone();
        session.manifest(self.time.wall()).write_atomic(&folder)
    }

    pub fn close(&self, id: &StreamId, why: &str) {
        let mut held = self.held();
        let Some(session) = held.as_mut() else { return };
        session.close(id, why);
        let folder = session.folder.clone();
        let _ = session.manifest(self.time.wall()).write_atomic(&folder);
    }

    /// Write one already-encoded frame.
    pub fn write(&self, id: &StreamId, frame: &[u8]) -> Result<(), String> {
        let mut held = self.held();
        let session = held.as_mut().ok_or("no recording is running")?;
        session.open.get_mut(id).ok_or("no such open stream")?.write(frame)
    }

    pub fn dropped(&self, id: &StreamId, count: u64, at: f64) {
        let mut held = self.held();
        let Some(session) = held.as_mut() else { return };
        let Some(stream) = session.open.get_mut(id) else { return };
        stream.dropped += count;
        stream.dropped_at = Some(at);
        let folder = session.folder.clone();
        let _ = session.manifest(self.time.wall()).write_atomic(&folder);
    }

    /// How full the stream's feeding buffer is, as its drain last saw it.
    pub fn fill(&self, id: &StreamId, fill: f32) {
        let mut held = self.held();
        if let Some(stream) = held.as_mut().and_then(|s| s.open.get_mut(id)) {
            stream.fill = fill;
        }
    }

    pub fn status(&self) -> Status {
        let held = self.held();
        let Some(session) = held.as_ref() else {
            return Status { running: false, folder: None, started: None, streams: Vec::new() };
        };
        Status {
            running: true,
            folder: Some(session.folder.clone()),
            started: Some(session.started),
            streams: session
                .open
                .iter()
                .map(|(id, s)| StreamStatus {
                    node: id.node.clone(),
                    slot: id.slot.clone(),
                    engine: id.engine,
                    file: s.file.clone(),
                    frames: s.frames,
                    dropped: s.dropped,
                    fill: s.fill,
                })
                .collect(),
        }
    }
}
