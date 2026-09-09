//! The manifest a reader folds a recording from. It is a PROJECTION of the session's live state,
//! minted at every rewrite, so no count lives both here and on an open stream.

use serde::Serialize;
use std::io::Write;
use std::path::Path;

#[derive(Serialize)]
pub struct Manifest {
    pub goofi: &'static str,
    pub name: String,
    pub patch: Option<String>,
    pub origin_utc: String,
    pub started_utc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_utc: Option<String>,
    pub streams: Vec<Entry>,
}

#[derive(Serialize, Clone)]
pub struct Entry {
    pub file: String,
    pub node: String,
    pub slot: String,
    pub uid: String,
    pub engine: &'static str,
    pub t0_utc: String,
    pub t0_patch: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sfreq: Option<f64>,
    pub timeline: &'static str,
    /// Seconds a DERIVED timeline last stood ahead of patch time — what an analyst subtracts to
    /// line it up with a measured one. Absent on a stream that reads the clock itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drift: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<usize>,
    /// The shape every frame of an array file had, which folds its flat values back in one line.
    /// Absent where the shape MOVED: the sidecar's own shape line is the index then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<Vec<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<(u32, u32)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<f64>,
    /// What a video stream's encoding costs, in words: it is the one stream that is not exact.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<goofi_core::record::VideoQuality>,
    pub frames: u64,
    pub dropped: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dropped_at: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closed_because: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Manifest {
    /// Write beside and rename, so a reader sees a whole manifest or none.
    pub fn write_atomic(&self, folder: &Path) -> Result<(), String> {
        let body = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let beside = folder.join("manifest.json.new");
        let mut file = std::fs::File::create(&beside).map_err(|e| e.to_string())?;
        file.write_all(&body).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&beside, folder.join("manifest.json")).map_err(|e| e.to_string())
    }
}
