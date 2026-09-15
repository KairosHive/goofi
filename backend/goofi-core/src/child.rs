//! The one way goofi runs a process: a child joins the session, watches a liveness pipe, leads
//! its own process group, is listed while it lives, logs what it prints, and dies with its [`Child`].

use std::io::{self, PipeReader, PipeWriter, Read};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

use crate::log::Source;
use crate::registry::{self, Kind, Lease};
use crate::worker::Worker;

/// Env var carrying the liveness pipe's read end: a unix fd number, or a Windows HANDLE.
pub const LIVENESS_ENV: &str = "GOOFI_PARENT_PIPE";

/// A process this one spawned, alive at most as long as this value.
pub struct Child {
    inner: std::process::Child,
    name: String,
    /// Never written to: holding this end open IS the signal, and its close — including one the
    /// OS does on a crash — is the EOF the child exits on.
    alive: Option<PipeWriter>,
    reaped: bool,
    /// The threads copying a logged stream into the log; joined once the process has ended.
    drains: Vec<Worker>,
    _lease: Lease,
}

/// Where one of a child's output streams goes.
pub enum Out {
    /// Into the process log, line by line, under the child's source: the default.
    Log,
    /// A pipe the owner reads — a protocol channel, or a message it will report itself.
    Pipe,
    Null,
    File(std::fs::File),
}

/// A child being described: its name, the log source its output is filed under, and its wiring.
pub struct Spawn<'a> {
    name: String,
    cmd: &'a mut Command,
    source: Source,
    stdin: bool,
    stdout: Out,
    stderr: Out,
}

/// Describe `cmd` as `name` — the words a reader of the inventory sees. By default stdin is
/// closed and both output streams go to the log under a source of that name.
pub fn run(name: impl Into<String>, cmd: &mut Command) -> Spawn<'_> {
    let name = name.into();
    let source = Source::component(&name);
    Spawn { name, cmd, source, stdin: false, stdout: Out::Log, stderr: Out::Log }
}

/// Spawn `cmd` as `name` with the default wiring.
pub fn spawn(name: impl Into<String>, cmd: &mut Command) -> io::Result<Child> {
    run(name, cmd).spawn()
}

impl Spawn<'_> {
    /// File what the child prints under `source` — a node's, say — instead of its name.
    pub fn source(mut self, source: Source) -> Self {
        self.source = source;
        self
    }

    /// Keep a pipe to the child's stdin for the owner to write.
    pub fn stdin_piped(mut self) -> Self {
        self.stdin = true;
        self
    }

    pub fn stdout(mut self, out: Out) -> Self {
        self.stdout = out;
        self
    }

    pub fn stderr(mut self, out: Out) -> Self {
        self.stderr = out;
        self
    }

    /// Start the child: the session, the liveness pipe and the process group are added here.
    pub fn spawn(self) -> io::Result<Child> {
        let Spawn { name, cmd, source, stdin, stdout, stderr } = self;
        if let Some(session) = crate::session::current() {
            cmd.env(crate::session::ENV, session);
        }
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(cmd, 0);
        cmd.stdin(if stdin { Stdio::piped() } else { Stdio::null() });
        let wire = |out: Out| match out {
            Out::Log | Out::Pipe => Stdio::piped(),
            Out::Null => Stdio::null(),
            Out::File(file) => Stdio::from(file),
        };
        let (log_out, log_err) = (matches!(stdout, Out::Log), matches!(stderr, Out::Log));
        cmd.stdout(wire(stdout)).stderr(wire(stderr));
        let armed = arm(cmd)?;
        let mut inner = cmd.spawn()?;
        let lease = registry::lease(Kind::Child, format!("{name} (pid {})", inner.id()));
        let mut drains = Vec::new();
        if log_out {
            drains.extend(inner.stdout.take().and_then(|out| drain(&name, out, source.clone(), "stdout")));
        }
        if log_err {
            drains.extend(inner.stderr.take().and_then(|err| drain(&name, err, source, "stderr")));
        }
        Ok(Child { inner, name, alive: Some(armed.into_writer()), reaped: false, drains, _lease: lease })
    }
}

fn drain(name: &str, from: impl Read + Send + 'static, source: Source, stream: &'static str) -> Option<Worker> {
    crate::worker::spawn(format!("{name} {stream}"), move || crate::log::drain(from, source, stream)).ok()
}

/// Run a one-shot tool as `name` to completion, its output captured, killed at `within`.
pub fn output(name: impl Into<String>, cmd: &mut Command, within: Duration) -> io::Result<Output> {
    let mut child = run(name, cmd).stdout(Out::Pipe).stderr(Out::Pipe).spawn()?;
    // Both pipes drained on threads of their own, so a tool that fills one while this waits on
    // the other cannot deadlock against its reader.
    let stdout = child.inner.stdout.take().and_then(reader);
    let stderr = child.inner.stderr.take().and_then(reader);
    let status = child.wait_within(within)?;
    let collect = |r: Option<crate::worker::Worker<Vec<u8>>>| r.and_then(|h| h.join().ok()).unwrap_or_default();
    let (stdout, stderr) = (collect(stdout), collect(stderr));
    let status = status.ok_or_else(|| {
        io::Error::new(io::ErrorKind::TimedOut, format!("{} did not finish in {within:?}", child.name))
    })?;
    Ok(Output { status, stdout, stderr })
}

fn reader(mut from: impl Read + Send + 'static) -> Option<Worker<Vec<u8>>> {
    crate::worker::spawn("goofi-child-output", move || {
        let mut bytes = Vec::new();
        let _ = from.read_to_end(&mut bytes);
        bytes
    })
    .ok()
}

impl Child {
    /// Ask the child's group to leave, wait `grace`, then insist. Returns how it ended, or `None`
    /// when it had to be killed.
    pub fn stop(&mut self, grace: Duration) -> Option<ExitStatus> {
        if self.reaped {
            return None;
        }
        // Closed first: this reaches a child that watches the pipe even where a signal or a dead
        // handle defeats the kill.
        drop(self.alive.take());
        let _ = request_stop(self.inner.id());
        let ended = self.poll(Instant::now() + grace);
        self.reaped = true;
        if ended.is_none() {
            let _ = force_kill(self.inner.id());
            let _ = self.inner.kill();
            let _ = self.inner.wait();
        }
        self.settle_log();
        ended
    }

    /// The last lines a dead child printed are in the log before its end is reported. A
    /// grandchild holding the pipe open is not waited for.
    fn settle_log(&mut self) {
        for drain in self.drains.drain(..) {
            let _ = drain.join_within(Duration::from_secs(1));
        }
    }

    /// Wait for a tool to finish, killing it at the deadline. `Ok(None)` is the deadline.
    pub fn wait_within(&mut self, within: Duration) -> io::Result<Option<ExitStatus>> {
        if self.reaped {
            return Ok(None);
        }
        let ended = self.poll(Instant::now() + within);
        self.reaped = true;
        if ended.is_none() {
            let _ = force_kill(self.inner.id());
            let _ = self.inner.kill();
            let _ = self.inner.wait();
        }
        self.settle_log();
        Ok(ended)
    }

    /// Wait for the child to end on its own, however long that takes.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.inner.wait()?;
        self.reaped = true;
        self.settle_log();
        Ok(status)
    }

    fn poll(&mut self, deadline: Instant) -> Option<ExitStatus> {
        loop {
            match self.inner.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
                _ => return None,
            }
        }
    }
}

impl std::ops::Deref for Child {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::DerefMut for Child {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl Drop for Child {
    /// No child outlives its owner: one still running is killed and reaped here.
    fn drop(&mut self) {
        if !self.reaped && self.inner.try_wait().ok().flatten().is_none() {
            drop(self.alive.take());
            let _ = force_kill(self.inner.id());
            let _ = self.inner.kill();
            let _ = self.inner.wait();
        }
        self.settle_log();
    }
}

// ---- the stop policy, shared with the PTY harness, which portable-pty spawns itself ----

/// Ask everything `pid` started to leave, so it can save its state on the way out. The pid names
/// a process GROUP leader: every child spawned here is one, and so is a PTY child.
#[cfg(unix)]
pub fn request_stop(pid: u32) -> Result<(), String> {
    signal(pid, libc::SIGTERM)
}

/// Insist, once the grace has run out.
#[cfg(unix)]
pub fn force_kill(pid: u32) -> Result<(), String> {
    signal(pid, libc::SIGKILL)
}

/// A process that has already left is what was being asked for, not a failure.
#[cfg(unix)]
fn signal(pid: u32, sig: i32) -> Result<(), String> {
    // SAFETY: a plain syscall whose only fallible argument is the pid, and that pid names a child
    // this process spawned and has not yet reaped.
    if unsafe { libc::kill(-(pid as i32), sig) } == 0 {
        return Ok(());
    }
    let err = io::Error::last_os_error();
    match err.raw_os_error() {
        Some(libc::ESRCH) => Ok(()),
        _ => Err(err.to_string()),
    }
}

/// A Windows console process refuses `taskkill` without `/F`; that is not a failure, because the
/// grace reaches [`force_kill`].
#[cfg(windows)]
pub fn request_stop(pid: u32) -> Result<(), String> {
    taskkill(pid, false).map(drop)
}

#[cfg(windows)]
pub fn force_kill(pid: u32) -> Result<(), String> {
    let out = taskkill(pid, true)?;
    if out.status.success() || out.status.code() == Some(NOT_FOUND) {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
}

/// taskkill's exit code for "there is no such pid" — the unix half's `ESRCH` under another name.
#[cfg(windows)]
const NOT_FOUND: i32 = 128;

/// `/T` is why this shells out rather than calling `TerminateProcess`: it takes the whole tree.
#[cfg(windows)]
fn taskkill(pid: u32, force: bool) -> Result<Output, String> {
    let mut cmd = Command::new("taskkill");
    cmd.args(["/T", "/PID"]).arg(pid.to_string());
    if force {
        cmd.arg("/F");
    }
    cmd.output().map_err(|e| format!("taskkill: {e}"))
}

// ---- the liveness pipe ----

/// The armed pipe, holding the read end open across the `spawn` that inherits it.
struct Armed {
    writer: PipeWriter,
    reader: PipeReader,
}

impl Armed {
    fn into_writer(self) -> PipeWriter {
        drop(self.reader);
        self.writer
    }
}

/// Create the liveness pipe, arrange for `cmd`'s child to inherit the read end, and name it in
/// [`LIVENESS_ENV`]. The returned value must outlive `cmd.spawn()`.
fn arm(cmd: &mut Command) -> io::Result<Armed> {
    let (reader, writer) = io::pipe()?;
    // Only the READ end is shared: std pipes are CLOEXEC, so the child's EOF means this
    // process died, not that a cousin still holds a write end.
    let value = share_read_end(cmd, &reader)?;
    cmd.env(LIVENESS_ENV, value);
    Ok(Armed { writer, reader })
}

/// Made inheritable HERE, not in a `pre_exec` hook: a hook forces fork-and-exec, whose child can
/// deadlock on a PATH search. A cousin inheriting the read end too is harmless: EOF is the writer's.
#[cfg(unix)]
fn share_read_end(_cmd: &mut Command, reader: &PipeReader) -> io::Result<String> {
    use std::os::fd::AsRawFd;

    let fd = reader.as_raw_fd();
    // SAFETY: a plain fcntl on a descriptor `reader` owns for the whole call.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, 0) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // A spawn copies the descriptor table verbatim, so the child sees the same number.
    Ok(fd.to_string())
}

// The Windows half is not CI-verified: this ran on no Windows host when it was written.
#[cfg(windows)]
fn share_read_end(_cmd: &mut Command, reader: &PipeReader) -> io::Result<String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};

    let handle = reader.as_raw_handle();
    // SAFETY: `handle` is live and owned by `reader` for the whole call; std passes
    // bInheritHandles=TRUE, so an inheritable handle crosses with the same numeric value.
    if unsafe { SetHandleInformation(handle as _, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((handle as usize).to_string())
}

#[cfg(unix)]
fn reader_from_raw(raw: &str) -> io::Result<std::fs::File> {
    use std::os::fd::FromRawFd;
    let fd: std::os::fd::RawFd =
        raw.parse().map_err(|_| io::Error::other(format!("{LIVENESS_ENV}=`{raw}` is not an fd")))?;
    // SAFETY: the parent handed us this fd across exec and closed its own copy.
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

#[cfg(windows)]
fn reader_from_raw(raw: &str) -> io::Result<std::fs::File> {
    use std::os::windows::io::FromRawHandle;
    let value: usize =
        raw.parse().map_err(|_| io::Error::other(format!("{LIVENESS_ENV}=`{raw}` is not a handle")))?;
    // SAFETY: the parent handed us this handle across the spawn and closed its own copy.
    Ok(unsafe { std::fs::File::from_raw_handle(value as _) })
}

/// The child side: end this process the moment the parent dies. A child with no pipe in its
/// environment was not started by goofi and is left alone.
pub fn watch_parent() -> io::Result<()> {
    let Ok(raw) = std::env::var(LIVENESS_ENV) else { return Ok(()) };
    let reader = reader_from_raw(&raw)?;
    std::thread::Builder::new().name("goofi-parent-watch".into()).spawn(move || {
        wait_for_parent_exit(reader);
        // Outliving the parent is not a failure, so exit clean.
        std::process::exit(0);
    })?;
    Ok(())
}

/// Block until the parent's write end closes. The parent never writes, so a byte is not a death.
fn wait_for_parent_exit(mut reader: impl Read) {
    let mut scratch = [0u8; 64];
    loop {
        match reader.read(&mut scratch) {
            Ok(0) => return,
            Ok(_) => continue,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        }
    }
}
