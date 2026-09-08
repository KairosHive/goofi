//! The sidecar every stream has: one line per frame, holding the instant it was made and whatever
//! of the `Meta` the file itself has nowhere to put has MOVED since the line before.
//!
//! It is JSON lines rather than a packed array of instants because a line is the unit a kill can
//! truncate to, and because the same file then serves a `.wav`, which carries no metadata at all,
//! and a `.npy`, which carries no time.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::{Duration, Instant};

const SYNC_EVERY: Duration = Duration::from_secs(1);

/// What a line says about the frame's extent in the file — the one thing a reader needs to split
/// a file whose frames are not all the same size. An array says its SHAPE, and only where that
/// moved, since a reader carries the last one forward; every other kind says how many of the
/// file's own units the frame added, because no shape can say it for them.
pub enum Extent<'a> {
    Shape(Option<&'a [usize]>),
    Rows(u64),
}

pub struct Beside {
    file: BufWriter<File>,
    /// The meta as the lines so far left it. An entry is written only where it MOVED, so `sfreq`
    /// and the channel names cost one line each rather than one per frame.
    said: goofi_core::Said,
    /// Reused, both of them: a line is written on the drain's own budget, so it allocates nothing.
    body: Vec<u8>,
    scratch: Vec<u8>,
    synced: Instant,
}

impl Beside {
    /// Open the sidecar for `file`, which is that file's own name with the suffix replaced.
    pub fn create(file: &Path) -> Result<Beside, String> {
        let path = file.with_extension("jsonl");
        let file = File::create_new(path).map_err(|e| e.to_string())?;
        Ok(Beside {
            file: BufWriter::new(file),
            said: goofi_core::Said::new(),
            body: Vec::new(),
            scratch: Vec::new(),
            synced: Instant::now(),
        })
    }

    /// One frame's line: when it was made, what it takes of the file, and whatever of its meta
    /// MOVED. The extent is what makes the sidecar an index — a `.wav` holds blocks of no fixed
    /// length and a flat `.npy` holds frames of no fixed shape, so without it nothing can say
    /// which samples belong to which instant.
    ///
    /// The meta is serialized STRAIGHT to the file: building a `Value` per frame first cost 4.3 µs
    /// a line against 0.33, and a fast stream outran its own drain.
    pub fn line(
        &mut self,
        at: f64,
        extent: Extent<'_>,
        meta: Option<&goofi_core::Meta>,
    ) -> Result<(), String> {
        write!(self.file, "{{\"t\":{at}").map_err(|e| e.to_string())?;
        match extent {
            Extent::Rows(rows) => write!(self.file, ",\"n\":{rows}").map_err(|e| e.to_string())?,
            Extent::Shape(Some(shape)) => {
                self.file.write_all(b",\"shape\":[").map_err(|e| e.to_string())?;
                for (i, d) in shape.iter().enumerate() {
                    let sep = if i == 0 { "" } else { "," };
                    write!(self.file, "{sep}{d}").map_err(|e| e.to_string())?;
                }
                self.file.write_all(b"]").map_err(|e| e.to_string())?;
            }
            Extent::Shape(None) => {}
        }
        if let Some(meta) = meta {
            self.body.clear();
            let wrote =
                goofi_core::write_meta_delta(&mut self.body, meta, &mut self.said, &mut self.scratch)
                    .map_err(|e| e.to_string())?;
            if wrote {
                self.file.write_all(b",\"meta\":").map_err(|e| e.to_string())?;
                self.file.write_all(&self.body).map_err(|e| e.to_string())?;
            }
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
