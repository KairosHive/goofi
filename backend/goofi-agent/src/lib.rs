//! The harness plane: the coding agents goofi launches, one PTY per instance. It is a session-wide
//! registry rather than one process's — a record per instance in the session directory and a
//! loopback socket on its owner — so any goofi process of the session (the server for its panel, a
//! node in a child of any tier) can spawn, list, attach to, type into or stop an instance, through
//! the one path. The owner keeps the PTY; every view, its own included, is a socket connection.

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::{Arc, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use goofi_supervisor::sync::Mutex;
use goofi_supervisor::{child, layout, session, timing, worker};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde_json::{json, Value};

/// How long a stopped harness has to leave on its own before it is killed outright.
pub const GRACE: Duration = Duration::from_secs(5);

/// How long a view waits for the PTY's end-of-stream after the child is reaped: ConPTY keeps the
/// pseudoconsole open past the exit, so there the exit frame goes after this settle instead.
const EXIT_SETTLE: Duration = Duration::from_millis(250);

/// How many PTY chunks a view may fall behind before it loses bytes; blocking the drain instead
/// would stall the child.
const OUTPUT_BACKLOG: usize = 1024;

/// How much output an attach replays — enough for a `command not found` and a stack trace.
const TAIL_BYTES: usize = 8192;

/// The largest frame a peer may announce; a longer length is a broken peer, not an allocation.
const FRAME_LIMIT: usize = 16 << 20;

/// How long the owner's workers are joined at shutdown.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(5);

/// The options a launch takes beside the entry's name: each becomes the entry's flag for it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Launch {
    pub model: Option<String>,
    pub effort: Option<String>,
}

impl Launch {
    /// The entry's command line with this launch's flags appended. An option the entry has no
    /// flag for is refused, as is a value that is not one bare word, since it is pasted into a
    /// shell line unquoted.
    fn command_line(&self, entry: &layout::Agent) -> Result<String, String> {
        let mut line = entry.command.clone();
        for (what, value, template) in [("model", &self.model, &entry.model), ("effort", &self.effort, &entry.effort)] {
            let Some(value) = value else { continue };
            let template = template.as_ref().ok_or_else(|| {
                format!("agent `{}` takes no {what}: its config entry has no `{what}` flag", entry.name)
            })?;
            let word = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '/' | '@');
            if value.is_empty() || !value.chars().all(word) {
                return Err(format!("a {what} is one word of letters, digits and `.-_:/@`, not `{value}`"));
            }
            line.push(' ');
            line.push_str(&template.replace("{}", value));
        }
        Ok(line)
    }
}

/// One instance as its record spells it: what `roster` answers in any process.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Record {
    pub id: String,
    pub harness: String,
    /// When it was spawned, in milliseconds since the epoch: what the roster is ordered by.
    pub born: u64,
    #[serde(flatten)]
    pub launch: Launch,
    /// `running`, `stopping` — a stop asked for but not yet obeyed is its OWN state, for the whole
    /// grace — or `exited`.
    pub state: String,
    pub exit_code: Option<u32>,
    /// The harness's pid, and the pid of the goofi process whose PTY it runs on.
    pub pid: u32,
    pub owner: u32,
    /// Where the owner serves this instance's views, on the loopback.
    pub port: u16,
}

/// One frame of a view's socket: the PTY's bytes in either direction, or a JSON control message.
/// Inbound control is `{op: resize, cols, rows}` (zero retracts), `{op: entry, text}` and
/// `{op: stop}`; outbound is `{op: size, cols, rows}` and, last of all, `{exit_code}`.
#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    Bytes(Vec<u8>),
    Control(Value),
}

impl Frame {
    /// Read one frame: a tag byte, a little-endian length, the payload. `None` at a clean end.
    pub fn read(from: &mut impl Read) -> std::io::Result<Option<Frame>> {
        let mut head = [0u8; 5];
        match from.read_exact(&mut head) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }
        let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
        if len > FRAME_LIMIT {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("a {len}-byte frame is over the limit")));
        }
        let mut payload = vec![0u8; len];
        from.read_exact(&mut payload)?;
        Ok(Some(match head[0] {
            0 => Frame::Bytes(payload),
            _ => Frame::Control(serde_json::from_slice(&payload).map_err(std::io::Error::other)?),
        }))
    }

    pub fn write(&self, to: &mut impl Write) -> std::io::Result<()> {
        let (tag, payload) = match self {
            Frame::Bytes(b) => (0u8, b.clone()),
            Frame::Control(v) => (1u8, v.to_string().into_bytes()),
        };
        let mut out = Vec::with_capacity(5 + payload.len());
        out.push(tag);
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&payload);
        to.write_all(&out)?;
        to.flush()
    }
}

/// A view of one instance: what an attach hands back. The tail the child wrote before this view
/// arrived comes first, then the live stream, the size whenever it settles, and `{exit_code}` as
/// the very last frame before the owner closes the connection.
pub struct View {
    reader: Mutex<TcpStream>,
    writer: Mutex<TcpStream>,
}

impl View {
    fn connect(record: &Record) -> Result<View, String> {
        let addr = SocketAddr::from(([127, 0, 0, 1], record.port));
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
            .map_err(|e| format!("harness instance `{}`: its owner does not answer: {e}", record.id))?;
        let _ = stream.set_nodelay(true);
        let writer = stream.try_clone().map_err(|e| e.to_string())?;
        Ok(View { reader: Mutex::new(stream), writer: Mutex::new(writer) })
    }

    /// The next frame, blocking; `None` once the owner has closed the connection. One reader at
    /// a time: the lock is held across the wait.
    pub fn read(&self) -> std::io::Result<Option<Frame>> {
        Frame::read(&mut *self.reader.lock())
    }

    pub fn write(&self, frame: &Frame) -> std::io::Result<()> {
        frame.write(&mut *self.writer.lock())
    }

    /// Keystrokes (or a paste) as typed.
    pub fn keys(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.write(&Frame::Bytes(bytes.to_vec()))
    }

    /// This view's word on the size; zero in either retracts it.
    pub fn resize(&self, cols: u16, rows: u16) -> std::io::Result<()> {
        self.write(&Frame::Control(json!({ "op": "resize", "cols": cols, "rows": rows })))
    }

    /// End the view: the reader wakes with `None`.
    pub fn close(&self) {
        let _ = self.writer.lock().shutdown(std::net::Shutdown::Both);
    }
}

/// The harness plane of one session.
pub struct Agents {
    session: String,
    /// The instances THIS process owns, in spawn order; an instance holds a weak hand on the list
    /// so a dismissal can take it off.
    owned: Arc<Mutex<Vec<Arc<Instance>>>>,
}

impl Agents {
    pub fn of(session: &str) -> Agents {
        Agents { session: session.to_string(), owned: Arc::default() }
    }

    /// The plane of the session this process runs under.
    pub fn current() -> Result<Agents, String> {
        session::current().map(|s| Agents::of(&s)).ok_or_else(|| "not inside a goofi session".to_string())
    }

    pub fn session(&self) -> &str {
        &self.session
    }

    /// The agents the config offers to launch, `_`-test entries withheld, and why a malformed
    /// config degraded to the default list when it did.
    pub fn offered() -> (Vec<layout::Agent>, Option<String>) {
        let (agents, error) = layout::agents();
        (agents.into_iter().filter(|a| !a.name.starts_with('_')).collect(), error)
    }

    /// The patch workspace of this session: where the manager mounts it, newest first when a load
    /// is mid-swap. What a spawn from a child runs its harness in.
    pub fn workspace(&self) -> Option<PathBuf> {
        let mut mounts: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(session::workspace_dir(&self.session))
            .ok()?
            .flatten()
            .map(|e| e.path().join("workspace"))
            .filter(|p| p.is_dir())
            .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
            .collect();
        mounts.sort();
        mounts.pop().map(|(_, p)| p)
    }

    fn records_dir(&self) -> PathBuf {
        session::system_dir(&self.session).join("agents")
    }

    /// Every instance of the session, in spawn order. A record whose owner has died is removed on
    /// the way, since nothing answers for it any more.
    pub fn roster(&self) -> Vec<Record> {
        let Ok(entries) = std::fs::read_dir(self.records_dir()) else { return Vec::new() };
        let mut found: Vec<Record> = entries
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| {
                let record: Record = serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok()?;
                if !child::alive(record.owner) {
                    let _ = std::fs::remove_file(e.path());
                    return None;
                }
                Some(record)
            })
            .collect();
        found.sort_by(|a, b| a.born.cmp(&b.born).then_with(|| a.id.cmp(&b.id)));
        found
    }

    /// The record of one instance, by id.
    pub fn record(&self, id: &str) -> Result<Record, String> {
        self.roster().into_iter().find(|r| r.id == id).ok_or_else(|| format!("no harness instance `{id}`"))
    }

    /// Launch the config entry `name` on a PTY under a LOGIN shell, with `cwd` as its working
    /// directory and `launch`'s options as the entry's flags; this process becomes its owner.
    /// `env` is the parent environment the terminal contract is laid over.
    pub fn spawn(&self, name: &str, launch: Launch, cwd: &Path, env: &[(OsString, OsString)]) -> Result<String, String> {
        let (agents, _) = layout::agents();
        let entry = agents.iter().find(|a| a.name == name).ok_or_else(|| {
            // The `_`-prefixed test entries are spawnable but never advertised.
            let have: Vec<&str> = agents.iter().map(|a| a.name.as_str()).filter(|n| !n.starts_with('_')).collect();
            match have.is_empty() {
                true => format!("unknown agent `{name}` — the config lists none; add [[agents]] to {}", layout::config_file().display()),
                false => format!("unknown agent `{name}` — the config offers: {}", have.join(", ")),
            }
        })?;
        let command = launch.command_line(entry)?;
        let id = session::nonce_hex(6)?;
        let exe = session::sessions().into_iter().find(|s| s.id == self.session).map(|s| s.exe).filter(|e| !e.is_empty())
            .ok_or_else(|| format!("session {} has no record of its binary", self.session))?;
        let own = Path::new(&exe).parent().map(Path::to_path_buf).ok_or("the session's binary has no directory")?;

        let pty = native_pty_system().openpty(PtySize::default()).map_err(|e| e.to_string())?;
        let mut cmd = shell_command(&command);
        cmd.cwd(cwd);
        // The parent environment, then the terminal contract ON TOP of it.
        for (k, v) in env {
            cmd.env(k, v);
        }
        for key in child::EMBEDDED_ONLY {
            cmd.env_remove(key);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        if !parent_speaks_utf8(env) {
            cmd.env("LC_ALL", "C.UTF-8");
        }
        // How the shell finds ITS server: the id names the session file, the actor its undo
        // stack, and the leading PATH entry makes `goofi` this server's own binary.
        cmd.env(session::ENV, &self.session);
        cmd.env("GOOFI_ACTOR", actor_of(&id));
        cmd.env("PATH", prepend_path(&own, env));
        // Git Bash's profile rebuilds PATH from ORIGINAL_PATH when a shell inherited one, and would
        // drop the entry above: an agent's `bash` tool then finds no `goofi`. Lead that one too.
        if let Some((_, original)) = env.iter().find(|(k, _)| k == "ORIGINAL_PATH") {
            cmd.env("ORIGINAL_PATH", format!("{}:{}", posix_form(&own), original.to_string_lossy()));
        }
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("a loopback port for the views: {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let child = pty.slave.spawn_command(cmd).map_err(|e| format!("spawn {name}: {e}"))?;
        // Closed here, or the master never sees EOF when the child exits and the drain blocks.
        drop(pty.slave);
        let pid = child.process_id().unwrap_or_default();
        let mut reader = pty.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pty.master.take_writer().map_err(|e| e.to_string())?;
        let inst = Arc::new(Instance {
            record: Mutex::new(Record {
                id: id.clone(),
                harness: name.to_string(),
                born: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or_default(),
                launch,
                state: "running".into(),
                exit_code: None,
                pid,
                owner: std::process::id(),
                port,
            }),
            path: self.records_dir().join(format!("{id}.json")),
            registry: Arc::downgrade(&self.owned),
            port,
            closing: AtomicBool::new(false),
            tree: child.process_id().and_then(|pid| child::Tree::by_pid(pid).ok()),
            writer: Mutex::new(writer),
            master: Mutex::new(pty.master),
            fanout: Mutex::default(),
            eof: goofi_supervisor::sync::Latch::default(),
            stopping: AtomicBool::new(false),
            forgotten: AtomicBool::new(false),
            paste_mode: AtomicBool::new(false),
            sizes: Mutex::default(),
            size: Mutex::new(None),
            seats: AtomicU64::new(0),
            workers: Mutex::default(),
            reaped: Mutex::new(None),
            _lease: goofi_supervisor::scope::lease(goofi_supervisor::scope::Kind::Child, format!("harness {name} (pid {pid})")),
        });
        inst.publish()?;

        // Drained unconditionally: a child whose output nobody reads blocks on a full buffer.
        let draining = inst.clone();
        let drain = worker::spawn("goofi-agent-drain", move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let bytes = answer_cursor_query(&draining, &buf[..n]);
                note_paste_mode(&draining, &bytes);
                draining.fanout.lock().push(Out::Bytes(bytes));
            }
            // AFTER the final push, so a view that waits on this was offered every byte.
            draining.eof.open();
        });
        inst.workers.lock().extend(drain);

        let serving = inst.clone();
        let accept = worker::spawn("goofi-agent-views", move || {
            for stream in listener.incoming().flatten() {
                if serving.closing.load(Ordering::Relaxed) {
                    break;
                }
                serving.view(stream);
            }
        });
        inst.workers.lock().extend(accept);

        // BEFORE the reaper starts, or a child that dies instantly leaves a roster this instance
        // is not yet on.
        self.owned.lock().push(inst.clone());
        let exited = inst.clone();
        let reaper = worker::spawn("goofi-agent-reap", move || {
            let mut child = child;
            let code = child.wait().map(|s| s.exit_code()).unwrap_or(1);
            // The exit is published FIRST — `wait` freed the pid, and the grace thread must see
            // it before it can aim a kill at a recycled process group.
            {
                let mut record = exited.record.lock();
                record.state = "exited".into();
                record.exit_code = Some(code);
            }
            let _ = exited.publish();
            // The exit frame is the LAST thing a view gets: `wait` returns while a dying harness's
            // final words are still in flight. Bounded, because ConPTY never closes the stream.
            exited.eof.wait_until(std::time::Instant::now() + EXIT_SETTLE);
            exited.fanout.lock().push(Out::Exit(code));
        });
        if let Ok(reaper) = reaper {
            *inst.reaped.lock() = Some(reaper.done());
            inst.workers.lock().push(reaper);
        }
        Ok(id)
    }

    /// Attach to an instance, this process's or another's: the socket a view is.
    pub fn attach(&self, id: &str) -> Result<View, String> {
        View::connect(&self.record(id)?)
    }

    /// Type `text` into a running instance as one entry — a steering prompt, as if typed at its
    /// input: wrapped as a paste when the child has asked for pastes to be marked, then Enter.
    pub fn send(&self, id: &str, text: &str) -> Result<(), String> {
        let record = self.record(id)?;
        if record.state == "exited" {
            return Err(format!("harness instance `{id}` has exited"));
        }
        self.control(&record, json!({ "op": "entry", "text": text }))
    }

    /// Stop a running instance (ask, then insist after [`GRACE`]), or dismiss an exited one. It
    /// returns as soon as the owner has the request, because a grace the caller waits out is not one.
    pub fn stop(&self, id: &str) -> Result<(), String> {
        let record = self.record(id)?;
        self.control(&record, json!({ "op": "stop" }))
    }

    /// One control frame to the owner, on a connection of its own; the owner's acknowledgement
    /// is the connection closing.
    fn control(&self, record: &Record, control: Value) -> Result<(), String> {
        let view = View::connect(record)?;
        view.write(&Frame::Control(control)).map_err(|e| format!("harness instance `{}`: {e}", record.id))?;
        let _ = view.writer.lock().shutdown(std::net::Shutdown::Write);
        while let Ok(Some(_)) = view.read() {}
        Ok(())
    }

    /// Signal every instance this process owns to leave and forget them HERE; the closure waits
    /// out the grace and then insists, on the caller's thread. `None` when nothing was running.
    #[must_use]
    pub fn reap_all(&self) -> Option<impl FnOnce() + Send + 'static> {
        let taken = std::mem::take(&mut *self.owned.lock());
        if taken.is_empty() {
            return None;
        }
        for inst in &taken {
            inst.stopping.store(true, Ordering::Relaxed);
            inst.forget();
            let _ = inst.signal(child::Tree::ask);
        }
        Some(move || {
            let grace = timing::Deadline::within(GRACE);
            for inst in &taken {
                if !inst.reaped_within(grace.left()) {
                    let _ = inst.signal(child::Tree::kill);
                }
            }
            let deadline = timing::Deadline::within(SHUTDOWN_WAIT);
            for inst in &taken {
                inst.close(deadline);
            }
        })
    }
}

/// What the drain and the reaper hand every view, in order.
#[derive(Clone)]
enum Out {
    Bytes(Vec<u8>),
    Size(u16, u16),
    Exit(u32),
}

/// The tail and the live views, under ONE lock: a view that snapshots the tail and subscribes in
/// one step meets the live stream with no byte lost or doubled.
#[derive(Default)]
struct Fanout {
    tail: Vec<u8>,
    /// Set once the exit frame went out: a view arriving after it gets the tail, then the exit.
    exited: Option<u32>,
    /// Each view's channel, its socket, which a close shuts so its reader wakes, and its writer's
    /// end, which a close waits on first so the exit frame goes out.
    views: Vec<(u64, SyncSender<Out>, TcpStream, Option<worker::Done>)>,
}

impl Fanout {
    fn push(&mut self, out: Out) {
        match &out {
            Out::Bytes(bytes) => {
                self.tail.extend_from_slice(bytes);
                let over = self.tail.len().saturating_sub(TAIL_BYTES);
                if over > 0 {
                    self.tail.drain(..over);
                }
            }
            Out::Exit(code) => self.exited = Some(*code),
            Out::Size(..) => {}
        }
        // A view that fell behind loses bytes rather than the CHILD stalling behind it; one whose
        // reader left is dropped, and so is one too far behind for the exit, so its writer ends.
        let last = matches!(out, Out::Exit(_));
        self.views.retain(|(_, view, _, _)| match view.try_send(out.clone()) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => !last,
            Err(TrySendError::Disconnected(_)) => false,
        });
    }
}

/// One spawned harness, as its owner holds it: the PTY, the views, the exit.
struct Instance {
    record: Mutex<Record>,
    /// Where the record is published, whole or absent.
    path: PathBuf,
    tree: Option<child::Tree>,
    /// Writing can block when the child stops reading, but it is bounded by what a person types,
    /// so it is taken inline.
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    /// The owner's list this instance is on, left on dismissal.
    registry: Weak<Mutex<Vec<Arc<Instance>>>>,
    port: u16,
    /// Set when the acceptor is to stop; the close connects once to wake it.
    closing: AtomicBool,
    fanout: Mutex<Fanout>,
    /// End-of-stream, which is NOT the exit: `child.wait()` returns while the words the child wrote
    /// on its way out are still in flight.
    eof: goofi_supervisor::sync::Latch,
    /// Set the moment a stop begins, ahead of the signal.
    stopping: AtomicBool,
    /// Set when its owner forgot it: the record is gone, and no later publish writes it back.
    forgotten: AtomicBool,
    /// Whether the child has asked for pastes to be bracketed, as a terminal emulator tracks it.
    paste_mode: AtomicBool,
    sizes: Mutex<Sizes>,
    size: Mutex<Option<(u16, u16)>>,
    seats: AtomicU64,
    /// Its entry in the resource index, for as long as the owner holds it.
    _lease: goofi_supervisor::scope::Lease,
    /// Its drain, its views' acceptor, its reaper and its stop: joined once it is dismissed.
    workers: Mutex<Vec<worker::Worker>>,
    /// The reaper's end, which is the exit: what a stop's grace waits on.
    reaped: Mutex<Option<worker::Done>>,
}

impl Instance {
    /// Write the record whole: the session-file idiom, so a reader sees all of it or none.
    fn publish(&self) -> Result<(), String> {
        let record = self.record.lock();
        if self.forgotten.load(Ordering::Relaxed) {
            return Ok(());
        }
        let mut record = record.clone();
        if record.state == "running" && self.stopping.load(Ordering::Relaxed) {
            record.state = "stopping".into();
        }
        let dir = self.path.parent().expect("a record lives in the agents directory");
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let part = self.path.with_extension("json.part");
        std::fs::write(&part, serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?)
            .and_then(|()| std::fs::rename(&part, &self.path))
            .map_err(|e| format!("{}: {e}", self.path.display()))
    }

    /// Take the record off the roster for good, under its lock so a publish in flight cannot
    /// write it back.
    fn forget(&self) {
        let _record = self.record.lock();
        self.forgotten.store(true, Ordering::Relaxed);
        let _ = std::fs::remove_file(&self.path);
    }

    /// Serve one connection as a view: a writer thread carries the tail, the live output, the size
    /// and the exit; the reader runs HERE and ends the view when the peer leaves.
    fn view(self: &Arc<Self>, stream: TcpStream) {
        let _ = stream.set_nodelay(true);
        let (Ok(mut writing), Ok(held)) = (stream.try_clone(), stream.try_clone()) else { return };
        let seat = self.seats.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = sync_channel::<Out>(OUTPUT_BACKLOG);
        let (tail, exited) = {
            let mut fanout = self.fanout.lock();
            fanout.views.push((seat, tx, held, None));
            (fanout.tail.clone(), fanout.exited)
        };
        let settled = *self.size.lock();
        let writer = worker::spawn("goofi-agent-view", move || {
            let send = |w: &mut TcpStream, f: Frame| f.write(w).is_ok();
            if !tail.is_empty() && !send(&mut writing, Frame::Bytes(tail)) {
                return;
            }
            // Sent up front: a view arriving on a settled size has no change event coming.
            if let Some((cols, rows)) = settled {
                if !send(&mut writing, size_frame(cols, rows)) {
                    return;
                }
            }
            if let Some(code) = exited {
                let _ = send(&mut writing, Frame::Control(json!({ "exit_code": code })));
                let _ = writing.shutdown(std::net::Shutdown::Write);
                return;
            }
            while let Ok(out) = rx.recv() {
                let ok = match out {
                    Out::Bytes(b) => send(&mut writing, Frame::Bytes(b)),
                    Out::Size(cols, rows) => send(&mut writing, size_frame(cols, rows)),
                    Out::Exit(code) => {
                        let _ = send(&mut writing, Frame::Control(json!({ "exit_code": code })));
                        false
                    }
                };
                if !ok {
                    break;
                }
            }
            let _ = writing.shutdown(std::net::Shutdown::Write);
        });
        if let Ok(writer) = &writer {
            if let Some(view) = self.fanout.lock().views.iter_mut().find(|v| v.0 == seat) {
                view.3 = Some(writer.done());
            }
        }
        let inst = self.clone();
        let reader = worker::spawn("goofi-agent-keys", move || {
            let mut reading = stream;
            while let Ok(Some(frame)) = Frame::read(&mut reading) {
                match frame {
                    Frame::Bytes(b) => inst.write(&b),
                    // A request — an entry or a stop — is answered by the connection closing.
                    Frame::Control(c) => if inst.control(seat, &c) { break },
                }
            }
            inst.leave(seat);
            inst.fanout.lock().views.retain(|(s, _, _, _)| *s != seat);
            let _ = reading.shutdown(std::net::Shutdown::Both);
        });
        // Every request is a connection of its own, so the finished ones are let go here.
        let mut workers = self.workers.lock();
        workers.retain(|w| !w.is_done());
        workers.extend(writer);
        workers.extend(reader);
    }

    /// An inbound control frame: a size proposal, an entry to type, or a stop. Answers whether
    /// the frame was a request the connection is done with.
    fn control(self: &Arc<Self>, seat: u64, c: &Value) -> bool {
        match c["op"].as_str() {
            Some("resize") => {
                let (cols, rows) = (c["cols"].as_u64().unwrap_or(0) as u16, c["rows"].as_u64().unwrap_or(0) as u16);
                self.propose(seat, (cols > 0 && rows > 0).then_some((cols, rows)));
                false
            }
            Some("entry") => {
                self.entry(c["text"].as_str().unwrap_or_default());
                true
            }
            Some("stop") => {
                if self.record.lock().exit_code.is_some() {
                    // Dismissal: the owner forgets it on a thread of its own, since this one is
                    // among the workers a dismissal joins.
                    let gone = self.clone();
                    let _ = worker::spawn("goofi-agent-dismiss", move || gone.dismiss());
                } else {
                    self.begin_stop();
                }
                true
            }
            _ => false,
        }
    }

    /// Ask the child to leave, and insist after the grace — a wait on the reap, so a child that
    /// leaves at once releases the thread at once.
    fn begin_stop(self: &Arc<Self>) {
        self.stopping.store(true, Ordering::Relaxed);
        let _ = self.publish();
        let _ = self.signal(child::Tree::ask);
        let stopped = self.clone();
        let stop = worker::spawn("goofi-agent-stop", move || {
            if !stopped.reaped_within(GRACE) {
                let _ = stopped.signal(child::Tree::kill);
            }
        });
        self.workers.lock().extend(stop);
    }

    /// One typed entry: the text, bracketed when the child marks pastes, then Enter.
    fn entry(&self, text: &str) {
        let mut bytes = Vec::with_capacity(text.len() + 16);
        match self.paste_mode.load(Ordering::Relaxed) {
            true => {
                bytes.extend_from_slice(PASTE_BEGIN);
                bytes.extend_from_slice(text.as_bytes());
                bytes.extend_from_slice(PASTE_END);
            }
            false => bytes.extend_from_slice(text.as_bytes()),
        }
        bytes.push(b'\r');
        self.write(&bytes);
    }

    /// This view's word on the size, `None` when it has nothing on screen. The lock is held across
    /// the settle, so two views resizing at once cannot land out of order.
    fn propose(&self, seat: u64, size: Option<(u16, u16)>) {
        let mut sizes = self.sizes.lock();
        self.settle(sizes.propose(seat, size));
    }

    /// This view is gone; the terminal goes back to whichever survivor spoke last.
    fn leave(&self, seat: u64) {
        let mut sizes = self.sizes.lock();
        self.settle(sizes.leave(seat));
    }

    /// Apply an arbitrated answer once, the kernel first; a `None` leaves the window where it was.
    fn settle(&self, now: Option<(u16, u16)>) {
        let Some((cols, rows)) = now else { return };
        let mut size = self.size.lock();
        if *size == now {
            return;
        }
        let _ = self.master.lock().resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        *size = now;
        self.fanout.lock().push(Out::Size(cols, rows));
    }

    /// Keystrokes (or a paste) from a view.
    fn write(&self, bytes: &[u8]) {
        let mut w = self.writer.lock();
        let _ = w.write_all(bytes).and_then(|()| w.flush());
    }

    /// Reach the tree with its ask or its kill, skipped once the child has been reaped, since a
    /// recycled pid would name a stranger.
    fn signal(&self, how: fn(&child::Tree) -> Result<(), String>) -> Result<(), String> {
        if self.record.lock().exit_code.is_some() {
            return Ok(());
        }
        how(self.tree.as_ref().ok_or("the harness reported no pid to signal")?)
    }

    /// Whether the child was reaped within `within`; one whose reaper is not up yet was not.
    fn reaped_within(&self, within: Duration) -> bool {
        let reaped = self.reaped.lock().clone();
        reaped.is_some_and(|done| done.wait_within(within))
    }

    /// Forget an exited instance: off its owner's list, its record gone, its port closed.
    fn dismiss(&self) {
        if let Some(registry) = self.registry.upgrade() {
            registry.lock().retain(|i| !std::ptr::eq(&**i, self));
        }
        self.forget();
        self.close(timing::Deadline::within(SHUTDOWN_WAIT));
    }

    /// Stop serving views and join the workers, every view's included, within the deadline.
    fn close(&self, deadline: timing::Deadline) {
        self.closing.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], self.port)), Duration::from_millis(100));
        // The senders go first: each writer delivers what it holds, the exit included, and ends.
        let views = std::mem::take(&mut self.fanout.lock().views);
        let (streams, writers): (Vec<_>, Vec<_>) = views.into_iter().map(|(_, _, s, w)| (s, w)).unzip();
        for writer in writers.into_iter().flatten() {
            writer.wait_within(deadline.left());
        }
        for stream in streams {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        for worker in std::mem::take(&mut *self.workers.lock()) {
            let _ = worker.join_within(deadline.left());
        }
    }
}

fn size_frame(cols: u16, rows: u16) -> Frame {
    Frame::Control(json!({ "op": "size", "cols": cols, "rows": rows }))
}

/// The terminal size every view of one instance shares: a PTY has ONE window, so the last view to
/// speak wins, and a view that retracts or leaves hands it back to the newest survivor.
#[derive(Default)]
struct Sizes {
    seats: Vec<(u64, Option<(u16, u16)>)>,
}

impl Sizes {
    /// `size` is `None` for a retraction. The seat moves to the end, so "last writer" means the
    /// last to have SPOKEN rather than the last to have arrived.
    fn propose(&mut self, seat: u64, size: Option<(u16, u16)>) -> Option<(u16, u16)> {
        self.seats.retain(|(s, _)| *s != seat);
        self.seats.push((seat, size));
        self.current()
    }

    fn leave(&mut self, seat: u64) -> Option<(u16, u16)> {
        self.seats.retain(|(s, _)| *s != seat);
        self.current()
    }

    fn current(&self) -> Option<(u16, u16)> {
        self.seats.iter().rev().find_map(|(_, s)| *s)
    }
}

/// Answer ConPTY's cursor-position query, which BLOCKS the child, but only while no view is
/// attached: xterm.js gives the real position, and a second reply is typed input.
fn answer_cursor_query(inst: &Instance, bytes: &[u8]) -> Vec<u8> {
    if !inst.fanout.lock().views.is_empty() {
        return bytes.to_vec();
    }
    let Some(stripped) = take_cursor_queries(bytes) else { return bytes.to_vec() };
    // Row 1, column 1 — a lie, told only when there is no screen to contradict it.
    inst.write(b"\x1b[1;1R");
    stripped
}

/// The bytes ConPTY sends to ask where the cursor is.
const CURSOR_QUERY: &[u8] = b"\x1b[6n";

/// Bracketed paste: what a child writes to turn the marking on and off, and the marks themselves.
const PASTE_ON: &[u8] = b"\x1b[?2004h";
const PASTE_OFF: &[u8] = b"\x1b[?2004l";
const PASTE_BEGIN: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

/// Keep the paste-mode flag at what the child's output last said, as a terminal emulator does.
fn note_paste_mode(inst: &Instance, bytes: &[u8]) {
    let last = |needle: &[u8]| bytes.windows(needle.len()).rposition(|w| w == needle);
    match (last(PASTE_ON), last(PASTE_OFF)) {
        (Some(on), off) if off.is_none_or(|off| off < on) => inst.paste_mode.store(true, Ordering::Relaxed),
        (_, Some(_)) => inst.paste_mode.store(false, Ordering::Relaxed),
        _ => {}
    }
}

/// `bytes` with every [`CURSOR_QUERY`] taken out, or `None` when there was none.
fn take_cursor_queries(bytes: &[u8]) -> Option<Vec<u8>> {
    if !bytes.windows(CURSOR_QUERY.len()).any(|w| w == CURSOR_QUERY) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some(at) = rest.windows(CURSOR_QUERY.len()).position(|w| w == CURSOR_QUERY) {
        out.extend_from_slice(&rest[..at]);
        rest = &rest[at + CURSOR_QUERY.len()..];
    }
    out.extend_from_slice(rest);
    Some(out)
}

/// The actor a spawned shell's ops carry: whose drags end with the instance.
pub fn actor_of(id: &str) -> String {
    format!("agent-{id}")
}

/// The instance an actor is the shell of, if it is one.
pub fn instance_of(actor: &str) -> Option<&str> {
    actor.strip_prefix("agent-")
}

/// The agent's command line under the user's own shell, as their terminal would run it: a LOGIN
/// shell on unix — nvm shims and profiles included — and `cmd /C` on Windows.
fn shell_command(command: &str) -> CommandBuilder {
    #[cfg(windows)]
    {
        let mut cmd = CommandBuilder::new("cmd");
        cmd.args(["/C", command]);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = CommandBuilder::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()));
        cmd.args(["-lc", command]);
        cmd
    }
}

/// A directory as an MSYS shell spells it: `C:\a\b` is `/c/a/b`. Elsewhere the path itself.
fn posix_form(dir: &Path) -> String {
    let text = dir.to_string_lossy().replace('\\', "/");
    match text.as_bytes() {
        [drive, b':', b'/', ..] if drive.is_ascii_alphabetic() => {
            format!("/{}{}", drive.to_ascii_lowercase() as char, &text[2..])
        }
        _ => text,
    }
}

/// The child's PATH with that directory FIRST. A login shell's profile may rebuild PATH over
/// this; `GOOFI_SESSION` still names the server, so a globally installed `goofi` also lands.
fn prepend_path(dir: &Path, env: &[(OsString, OsString)]) -> OsString {
    let tail = env
        .iter()
        // Case-insensitively: Windows stores the name as `Path`, and its own lookup ignores case.
        .find(|(k, _)| k.to_string_lossy().eq_ignore_ascii_case("PATH"))
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_default();
    std::env::join_paths(std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&tail)))
        .unwrap_or(tail)
}

/// The environment a spawned harness inherits: this process's own, whole.
pub fn parent_env() -> Vec<(OsString, OsString)> {
    std::env::vars_os().collect()
}

/// Whether what the child will SEE names a UTF-8 locale, in the C library's precedence.
fn parent_speaks_utf8(env: &[(OsString, OsString)]) -> bool {
    ["LC_ALL", "LC_CTYPE", "LANG"].iter()
        .find_map(|k| {
            env.iter()
                .find(|(n, v)| n.as_os_str() == OsStr::new(k) && !v.is_empty())
                .map(|(_, v)| v.to_string_lossy().into_owned())
                .or_else(|| std::env::var(k).ok().filter(|v| !v.is_empty()))
        })
        .is_some_and(|v| v.to_ascii_uppercase().replace('-', "").contains("UTF8"))
}
