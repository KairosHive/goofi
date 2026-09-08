//! The sidecar every stream has: one line per frame, holding the instant it was made and the
//! `Meta` the file itself has nowhere to put.
//!
//! It is JSON lines rather than a packed array of instants because a line is the unit a kill can
//! truncate to, and because the same file then serves a `.wav`, which carries no metadata at all,
//! and a `.npy`, which carries no time.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::{Duration, Instant};

const SYNC_EVERY: Duration = Duration::from_secs(1);

pub struct Beside {
    file: BufWriter<File>,
    synced: Instant,
}

impl Beside {
    /// Open the sidecar for `file`, which is that file's own name with the suffix replaced.
    pub fn create(file: &Path) -> Result<Beside, String> {
        let path = file.with_extension("jsonl");
        let file = File::create_new(path).map_err(|e| e.to_string())?;
        Ok(Beside { file: BufWriter::new(file), synced: Instant::now() })
    }

    /// One frame's line: when it was made, how many ROWS of the file it put there, and its meta.
    /// The row count is what makes the sidecar an index — a `.wav` holds blocks of no fixed
    /// length, so without it nothing can say which samples belong to which instant.
    ///
    /// The meta is serialized STRAIGHT to the file: building a `Value` per frame first cost 4.3 µs
    /// a line against 0.33, and a fast stream outran its own drain.
    pub fn line(&mut self, at: f64, rows: u64, meta: Option<&goofi_core::Meta>) -> Result<(), String> {
        write!(self.file, "{{\"t\":{at},\"n\":{rows}").map_err(|e| e.to_string())?;
        if let Some(meta) = meta {
            write!(self.file, ",\"meta\":").map_err(|e| e.to_string())?;
            serde_json::to_writer(&mut self.file, &goofi_core::MetaJson(meta))
                .map_err(|e| e.to_string())?;
        }
        writeln!(self.file, "}}").map_err(|e| e.to_string())?;
        if self.synced.elapsed() >= SYNC_EVERY {
            self.sync()?;
        }
        Ok(())
    }

    pub fn sync(&mut self) -> Result<(), String> {
        self.file.flush().map_err(|e| e.to_string())?;
        self.file.get_ref().sync_data().map_err(|e| e.to_string())?;
        self.synced = Instant::now();
        Ok(())
    }
}
