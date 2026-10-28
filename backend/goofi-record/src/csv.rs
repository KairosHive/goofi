//! A `.csv` a TABLE stream is appended to. A table is named columns of numbers, which is what a
//! CSV is, so this is the one stream kind whose file needs no interpretation at all.
//!
//! The columns are the first frame's; a frame that brings different ones is a different table and
//! opens a new file.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use goofi_core::record::Frame;
use goofi_core::Value;

use crate::stream::{Added, Format};

pub struct Csv {
    file: BufWriter<File>,
    columns: Vec<String>,
    rows: u64,
}

impl Csv {
    pub fn create(path: &Path, columns: &[String]) -> Result<Csv, String> {
        let file = File::create_new(path).map_err(|e| e.to_string())?;
        let mut csv = Csv { file: BufWriter::new(file), columns: columns.to_vec(), rows: 0 };
        let head: Vec<String> = std::iter::once("t".to_string()).chain(csv.columns.iter().map(|c| quoted(c))).collect();
        writeln!(csv.file, "{}", head.join(",")).map_err(|e| e.to_string())?;
        Ok(csv)
    }

}

impl Format for Csv {
    /// One frame as one row. A column the frame does not hold is empty rather than absent, so
    /// every row has the same width as the header.
    fn write(&mut self, frame: &Frame, at: f64) -> Result<Added, String> {
        let Frame::Data(d) = frame else { return Err("the frame is not the shape this stream holds".into()) };
        let Value::Table(map) = d.value() else { return Err("the frame is not the shape this stream holds".into()) };
        let mut row = vec![at.to_string()];
        for column in &self.columns {
            row.push(map.get(column).map(|child| quoted(&cell(child))).unwrap_or_default());
        }
        writeln!(self.file, "{}", row.join(",")).map_err(|e| e.to_string())?;
        self.rows += 1;
        Ok(Added::Rows(1))
    }

    fn frames(&self) -> u64 {
        self.rows
    }

    fn sync(&mut self) -> Result<(), String> {
        crate::stream::sync_file(&mut self.file)
    }

    fn finish(&mut self) -> Result<(), String> {
        self.sync()
    }
}

/// A table's cell as text. A column holding one number is that number; anything wider is the
/// shape it has, because a CSV cell holds one value and a reader must not be told otherwise.
fn cell(d: &goofi_core::Data) -> String {
    match d.value() {
        Value::Str(s) => s.to_string(),
        Value::Table(_) => "<table>".into(),
        Value::Texture(_) => "<texture submission>".into(),
        Value::Functional(_) => "<functional>".into(),
        Value::Array(a) => match (a.shape().iter().product::<usize>(), a.values().next()) {
            (1, Some(v)) => v.to_string(),
            _ => format!("<{:?}>", a.shape()),
        },
    }
}

/// RFC 4180: a field holding a comma, a quote or a newline is quoted, and its quotes are doubled.
fn quoted(v: &str) -> String {
    match v.contains([',', '"', '\n', '\r']) {
        false => v.to_string(),
        true => format!("\"{}\"", v.replace('"', "\"\"")),
    }
}
