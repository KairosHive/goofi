//! A `.csv` a TABLE stream is appended to. A table is named columns of numbers, which is what a
//! CSV is, so this is the one stream kind whose file needs no interpretation at all.
//!
//! The columns are the first frame's; a frame that brings different ones is a different table and
//! opens a new file, the way a reshaped array does.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub struct Csv {
    file: BufWriter<File>,
    columns: Vec<String>,
    rows: u64,
}

impl Csv {
    pub fn create(path: &Path, columns: &[String]) -> Result<Csv, String> {
        let file = File::create_new(path).map_err(|e| e.to_string())?;
        let mut csv = Csv { file: BufWriter::new(file), columns: columns.to_vec(), rows: 0 };
        let head: Vec<&str> = std::iter::once("t").chain(csv.columns.iter().map(String::as_str)).collect();
        writeln!(csv.file, "{}", head.join(",")).map_err(|e| e.to_string())?;
        Ok(csv)
    }

    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// One frame as one row. A column the frame does not hold is empty rather than absent, so
    /// every row has the same width as the header.
    pub fn write(&mut self, at: f64, cells: &[(String, String)]) -> Result<(), String> {
        let mut row = vec![at.to_string()];
        for column in &self.columns {
            let held = cells.iter().find(|(k, _)| k == column);
            row.push(held.map(|(_, v)| quoted(v)).unwrap_or_default());
        }
        writeln!(self.file, "{}", row.join(",")).map_err(|e| e.to_string())?;
        self.rows += 1;
        Ok(())
    }

    pub fn rows(&self) -> u64 {
        self.rows
    }

    pub fn sync(&mut self) -> Result<(), String> {
        self.file.flush().map_err(|e| e.to_string())?;
        self.file.get_ref().sync_data().map_err(|e| e.to_string())
    }
}

/// RFC 4180: a field holding a comma, a quote or a newline is quoted, and its quotes are doubled.
fn quoted(v: &str) -> String {
    match v.contains([',', '"', '\n', '\r']) {
        false => v.to_string(),
        true => format!("\"{}\"", v.replace('"', "\"\"")),
    }
}
