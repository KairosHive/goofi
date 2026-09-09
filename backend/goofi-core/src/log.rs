//! Process logs, grouped by source, level, stream and exact text.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::sync::{Mutex, OnceLock};
use serde::Serialize;

pub const MAX_GROUPS: usize = 10_000;
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_TEXT: usize = 64 * 1024;

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level { Info, Warning, Error }

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize)]
pub struct Source {
    pub component: String,
    pub node: Option<String>,
}

impl Source {
    pub fn component(name: &str) -> Self { Self { component: name.into(), node: None } }
}

thread_local! {
    static SOURCE: std::cell::RefCell<Source> = std::cell::RefCell::new(Source::component("goofi"));
}

pub fn source() -> Source { SOURCE.with(|s| s.borrow().clone()) }

/// Set the source for a dedicated node thread before its module is loaded.
pub fn set_source(source: Source) { SOURCE.with(|s| *s.borrow_mut() = source); }

#[derive(Clone, Debug, Hash, PartialEq, Eq, Serialize)]
pub struct Message {
    #[serde(flatten)]
    pub source: Source,
    pub level: Level,
    pub stream: Option<String>,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Group {
    pub id: u64,
    pub seq: u64,
    pub count: u64,
    pub ts: u64,
    #[serde(flatten)]
    pub message: Message,
}

#[derive(Serialize)]
pub struct Update {
    pub id: u64,
    pub seq: u64,
    pub count: u64,
    pub ts: u64,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
}

#[derive(Serialize)]
pub struct Batch {
    pub cursor: u64,
    pub oldest: u64,
    pub reset: bool,
    pub groups: Vec<Update>,
}

#[derive(Default)]
pub struct Log {
    seq: u64,
    bytes: usize,
    groups: BTreeMap<u64, Group>,
    keys: HashMap<Message, u64>,
}

impl Log {
    pub fn record(&mut self, mut message: Message) {
        if message.text.len() > MAX_TEXT {
            let mut end = MAX_TEXT;
            while !message.text.is_char_boundary(end) { end -= 1; }
            message.text.truncate(end);
            message.text.push_str("\n[message truncated]");
        }
        self.seq += 1;
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default().as_millis() as u64;
        let group = if let Some(old) = self.keys.get(&message).and_then(|seq| self.groups.remove(seq)) {
            Group { seq: self.seq, count: old.count + 1, ts, ..old }
        } else {
            self.bytes += message.text.len();
            Group { id: self.seq, seq: self.seq, count: 1, ts, message: message.clone() }
        };
        self.keys.insert(message, self.seq);
        self.groups.insert(self.seq, group);
        while self.groups.len() > MAX_GROUPS || self.bytes > MAX_BYTES {
            if let Some((_, old)) = self.groups.pop_first() {
                self.bytes -= old.message.text.len();
                self.keys.remove(&old.message);
            }
        }
    }

    /// Counters are absolute, so repeated delivery cannot count a message twice.
    pub fn since(&self, cursor: Option<u64>) -> Batch {
        let after = cursor.unwrap_or(0);
        Batch {
            cursor: self.seq,
            oldest: self.groups.first_key_value().map_or(self.seq + 1, |(seq, _)| *seq),
            reset: cursor.is_none(),
            groups: self.groups.range((std::ops::Bound::Excluded(after), std::ops::Bound::Unbounded))
                .map(|(_, g)| Update {
                    id: g.id, seq: g.seq, count: g.count, ts: g.ts,
                    message: (cursor.is_none() || g.id > after).then(|| g.message.clone()),
                }).collect(),
        }
    }
}

pub fn global() -> &'static Mutex<Log> {
    static LOG: OnceLock<Mutex<Log>> = OnceLock::new();
    LOG.get_or_init(|| Mutex::new(Log::default()))
}

pub fn record(source: Source, level: Level, stream: Option<&str>, text: impl Into<String>) {
    global().lock().unwrap_or_else(|e| e.into_inner()).record(Message {
        source, level, stream: stream.map(str::to_string), text: text.into(),
    });
}

/// Drain bytes without allowing an unterminated line to grow without limit.
pub fn drain(mut reader: impl Read, source: Source, stream: &str) {
    let mut pending = Vec::new();
    let mut bytes = [0; 4096];
    let emit = |bytes: &[u8]| {
        record(source.clone(), if stream == "stderr" { Level::Error } else { Level::Info },
            Some(stream), String::from_utf8_lossy(bytes).trim_end_matches('\r').to_string());
    };
    loop {
        match reader.read(&mut bytes) {
            Ok(0) => break,
            Ok(n) => for byte in &bytes[..n] {
                if *byte == b'\n' { emit(&pending); pending.clear(); }
                else {
                    pending.push(*byte);
                    if pending.len() == MAX_TEXT { emit(&pending); pending.clear(); }
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => { record(source.clone(), Level::Error, None, format!("Could not read {stream}: {e}")); break; }
        }
    }
    if !pending.is_empty() { emit(&pending); }
}

/// Capture native writes too. The saved terminal keeps startup prompts and the launch URL visible.
pub fn capture_stdio() -> Result<(), String> {
    use filedescriptor::{FileDescriptor, Pipe, StdioDescriptor};
    static CAPTURE: OnceLock<Result<(), String>> = OnceLock::new();
    CAPTURE.get_or_init(|| {
        let terminal = FileDescriptor::dup(&std::io::stdout()).map_err(|e| e.to_string())?;
        let _ = TERMINAL.set(Mutex::new(terminal));
        for (stream, descriptor) in [("stdout", StdioDescriptor::Stdout), ("stderr", StdioDescriptor::Stderr)] {
            let pipe = Pipe::new().map_err(|e| e.to_string())?;
            std::thread::Builder::new().name(format!("goofi-{stream}"))
                .spawn(move || drain(pipe.read, Source::component("goofi"), stream)).map_err(|e| e.to_string())?;
            let original = FileDescriptor::redirect_stdio(&pipe.write, descriptor).map_err(|e| e.to_string())?;
            #[cfg(windows)]
            {
                // SetStdHandle covers Rust and child processes. The C runtime has its own
                // descriptor table, used by Python extensions and native plugins.
                std::mem::forget(original);
                redirect_crt(&pipe.write, if stream == "stdout" { 1 } else { 2 })?;
            }
            #[cfg(not(windows))]
            drop(original);
        }
        Ok(())
    }).clone()
}

static TERMINAL: OnceLock<Mutex<filedescriptor::FileDescriptor>> = OnceLock::new();

pub fn terminal_line(text: &str) -> std::io::Result<()> {
    match TERMINAL.get() {
        Some(out) => writeln!(out.lock().unwrap_or_else(|e| e.into_inner()), "{text}"),
        None => { println!("{text}"); Ok(()) }
    }
}

pub fn print_url(url: &str) -> std::io::Result<()> { terminal_line(&format!("goofi → {url}")) }


#[cfg(windows)]
fn redirect_crt(writer: &filedescriptor::FileDescriptor, target: i32) -> Result<(), String> {
    use std::os::windows::io::{AsRawHandle, IntoRawHandle};
    let file = writer.as_file().map_err(|e| e.to_string())?;
    // On success the CRT owns this duplicate handle. On failure `file` still owns it.
    let fd = unsafe { libc::open_osfhandle(file.as_raw_handle() as isize, libc::O_BINARY) };
    if fd < 0 { return Err(std::io::Error::last_os_error().to_string()); }
    let _ = file.into_raw_handle();
    let result = unsafe { libc::dup2(fd, target) };
    let error = std::io::Error::last_os_error();
    unsafe { libc::close(fd); }
    if result < 0 { Err(error.to_string()) } else { Ok(()) }
}
