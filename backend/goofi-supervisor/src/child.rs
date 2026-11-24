//! The one way goofi runs a process: a child joins the session, watches a liveness pipe, leads
//! its own [`Tree`], is listed while it lives, logs what it prints, and dies with its [`Child`].

use std::io::{self, PipeReader, PipeWriter, Read};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::time::Duration;

use crate::log::Source;
use crate::scope::{self, Kind, Lease};
use crate::worker::Worker;

/// Env var carrying the liveness pipe's read end: a unix fd number, or a Windows HANDLE.
pub const LIVENESS_ENV: &str = "GOOFI_PARENT_PIPE";

/// Set in this process for the embedded interpreter, or by the user for their shell; no child
/// inherits them, so a host's path never reaches a node's interpreter.
pub const EMBEDDED_ONLY: [&str; 2] = ["PYTHONHOME", "PYTHONPATH"];

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
    tree: Tree,
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
    windowless: bool,
}

/// Describe `cmd` as `name` — the words a reader of the inventory sees. By default stdin is
/// closed and both output streams go to the log under a source of that name.
pub fn run(name: impl Into<String>, cmd: &mut Command) -> Spawn<'_> {
    let name = name.into();
    let source = Source::component(&name);
    Spawn { name, cmd, source, stdin: false, stdout: Out::Log, stderr: Out::Log, windowless: false }
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

    /// Give a console child no window of its own: one goofi runs with no console would show one.
    pub fn windowless(mut self) -> Self {
        self.windowless = true;
        self
    }

    /// Start the child: the session, the liveness pipe and the process group are added here.
    pub fn spawn(self) -> io::Result<Child> {
        let Spawn { name, cmd, source, stdin, stdout, stderr, windowless } = self;
        if let Some(session) = crate::session::id() {
            cmd.env(crate::session::ENV, session);
        }
        // A child that needs one of them states it on `cmd`.
        for key in EMBEDDED_ONLY {
            if !cmd.get_envs().any(|(k, _)| k == key) {
                cmd.env_remove(key);
            }
        }
        // Every pipe between goofi and a child carries UTF-8. A Python child follows the ANSI code
        // page on Windows unless told so; every other child ignores the variable.
        cmd.env("PYTHONUTF8", "1");
        // Its own process group, so a Ctrl+C at the terminal reaches goofi alone, which then
        // stops its children in order.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(cmd, 0);
        #[cfg(windows)]
        {
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let window = if windowless { CREATE_NO_WINDOW } else { 0 };
            std::os::windows::process::CommandExt::creation_flags(cmd, CREATE_NEW_PROCESS_GROUP | window);
        }
        #[cfg(not(windows))]
        let _ = windowless;
        cmd.stdin(if stdin { Stdio::piped() } else { Stdio::null() });
        let wire = |out: Out| match out {
            Out::Log | Out::Pipe => Stdio::piped(),
            Out::Null => Stdio::null(),
            Out::File(file) => Stdio::from(file),
        };
        let (log_out, log_err) = (matches!(stdout, Out::Log), matches!(stderr, Out::Log));
        cmd.stdout(wire(stdout)).stderr(wire(stderr));
        let (alive, _shared) = arm(cmd)?;
        let mut inner = cmd.spawn()?;
        let tree = Tree::of(&inner)?;
        let lease = scope::lease(Kind::Child, format!("{name} (pid {})", inner.id()));
        let mut drains = Vec::new();
        if log_out {
            drains.extend(inner.stdout.take().and_then(|out| drain(&name, out, source.clone(), "stdout")));
        }
        if log_err {
            drains.extend(inner.stderr.take().and_then(|err| drain(&name, err, source, "stderr")));
        }
        Ok(Child { inner, name, alive: Some(alive), reaped: false, drains, tree, _lease: lease })
    }
}

fn drain(name: &str, from: impl Read + Send + 'static, source: Source, stream: &'static str) -> Option<Worker> {
    crate::worker::spawn(format!("{name} {stream}"), move || crate::log::drain(from, source, stream)).ok()
}

/// Run a one-shot tool as `name` to completion, its output captured on the same deadline.
pub fn output(name: impl Into<String>, cmd: &mut Command, within: Duration) -> io::Result<Output> {
    let mut child = run(name, cmd).stdout(Out::Pipe).stderr(Out::Pipe).spawn()?;
    let deadline = crate::timing::Deadline::within(within);
    // Both pipes drained on threads of their own, so a tool that fills one while this waits on
    // the other cannot deadlock against its reader.
    let stdout = reader(child.inner.stdout.take().expect("piped stdout"))?;
    let stderr = reader(child.inner.stderr.take().expect("piped stderr"))?;
    let status = child.wait_within(deadline.left());
    let timeout = || {
        // A tool may have exited while its descendants still hold its output pipes.
        let _ = child.tree.kill();
        io::Error::new(io::ErrorKind::TimedOut, format!("{} did not finish in {within:?}", child.name))
    };
    let collect = |r: Worker<io::Result<Vec<u8>>>| {
        r.join_within(deadline.left())
            .ok_or_else(&timeout)?
            .map_err(|_| io::Error::other("the tool output reader panicked"))?
    };
    let (stdout, stderr) = (collect(stdout)?, collect(stderr)?);
    let status = status.ok_or_else(timeout)?;
    Ok(Output { status, stdout, stderr })
}

fn reader(mut from: impl Read + Send + 'static) -> io::Result<Worker<io::Result<Vec<u8>>>> {
    crate::worker::spawn("goofi-child-output", move || {
        let mut bytes = Vec::new();
        from.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

impl Child {
    /// Ask the child's tree to leave, wait `grace`, then insist. Returns how it ended, or `None`
    /// when it had to be killed.
    pub fn stop(&mut self, grace: Duration) -> Option<ExitStatus> {
        if !self.reaped {
            // Closed first: this reaches a child that watches the pipe even where a signal or a
            // dead handle defeats the kill.
            drop(self.alive.take());
            let _ = self.tree.ask();
        }
        self.wait_within(grace)
    }

    fn kill_and_reap(&mut self) {
        let _ = self.tree.kill();
        let _ = self.inner.kill();
        let _ = self.inner.wait();
    }

    /// The last lines a dead child printed are in the log before its end is reported. A
    /// grandchild holding the pipe open is not waited for.
    fn settle_log(&mut self) {
        for drain in self.drains.drain(..) {
            let _ = drain.join_within(Duration::from_secs(1));
        }
    }

    /// Wait for a tool to finish, killing it at the deadline. `None` is the deadline.
    pub fn wait_within(&mut self, within: Duration) -> Option<ExitStatus> {
        if self.reaped {
            return None;
        }
        let ended = self.poll(crate::timing::Deadline::within(within));
        self.reaped = true;
        if ended.is_none() {
            self.kill_and_reap();
        }
        self.settle_log();
        ended
    }

    /// The child's end, if it comes within `within`; one still running at the deadline is left
    /// running. What a peer that closed its doorbell is asked: gone, with its status, or going.
    pub fn ended_within(&mut self, within: Duration) -> Option<ExitStatus> {
        if self.reaped {
            return None;
        }
        let ended = self.poll(crate::timing::Deadline::within(within))?;
        self.reaped = true;
        self.settle_log();
        Some(ended)
    }

    /// Wait for the child to end on its own, however long that takes.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.inner.wait()?;
        self.reaped = true;
        self.settle_log();
        Ok(status)
    }

    /// The child's end, or `None` at the deadline. A timed wait on the process where the OS has
    /// one; a 10 ms ask elsewhere.
    fn poll(&mut self, deadline: crate::timing::Deadline) -> Option<ExitStatus> {
        loop {
            match self.inner.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) => {
                    let left = deadline.left();
                    if left.is_zero() {
                        return self.inner.try_wait().ok().flatten();
                    }
                    self.park(left);
                }
                Err(_) => return None,
            }
        }
    }

    /// Park up to `within` for the child to end, or 10 ms where the OS has no timed wait.
    #[cfg(target_os = "linux")]
    fn park(&self, within: Duration) {
        // SAFETY: `pidfd_open` on a child this process spawned and has not reaped; the fd is
        // polled once and closed here.
        let pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, self.inner.id() as libc::pid_t, 0) };
        if pidfd < 0 {
            std::thread::sleep(within.min(Duration::from_millis(10)));
            return;
        }
        let mut fds = libc::pollfd { fd: pidfd as i32, events: libc::POLLIN, revents: 0 };
        unsafe {
            libc::poll(&mut fds, 1, within.as_millis().min(i32::MAX as u128) as i32);
            libc::close(pidfd as i32);
        }
    }

    #[cfg(windows)]
    fn park(&self, within: Duration) {
        use std::os::windows::io::AsRawHandle;
        // SAFETY: a wait on the process handle std holds for this child, bounded by `within`.
        unsafe {
            windows_sys::Win32::System::Threading::WaitForSingleObject(
                self.inner.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE,
                within.as_millis().min(u32::MAX as u128) as u32,
            );
        }
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    fn park(&self, within: Duration) {
        std::thread::sleep(within.min(Duration::from_millis(10)));
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
            self.kill_and_reap();
        }
        self.settle_log();
    }
}

// ---- the stop policy, shared with the PTY harness, which portable-pty spawns itself ----

/// Everything one child started, as one thing to ask and to kill. On unix it is the process group
/// the child leads. On Windows it is a job object the child and every descendant stay in, killed
/// as one when the job closes — a `taskkill /T` walk misses a grandchild whose parent already
/// left — and the console group the child leads, which carries the ask.
pub struct Tree {
    pid: u32,
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
}

// SAFETY: a job handle is a kernel object reference, usable from any thread.
#[cfg(windows)]
unsafe impl Send for Tree {}
#[cfg(windows)]
unsafe impl Sync for Tree {}

impl Tree {
    /// The tree a child of this process leads: `cmd.spawn()`'s result.
    pub fn of(child: &std::process::Child) -> io::Result<Tree> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            Tree::adopt(child.id(), child.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE)
        }
        #[cfg(not(windows))]
        Ok(Tree { pid: child.id() })
    }

    /// The tree a process another spawner started leads — a PTY child — by pid. Every child spawned
    /// here leads a group, and so does a PTY child.
    pub fn by_pid(pid: u32) -> io::Result<Tree> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};
            // SAFETY: a handle opened on a pid, checked and closed below.
            let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid) };
            if process.is_null() {
                return Err(io::Error::last_os_error());
            }
            let tree = Tree::adopt(pid, process);
            unsafe { windows_sys::Win32::Foundation::CloseHandle(process) };
            tree
        }
        #[cfg(not(windows))]
        Ok(Tree { pid })
    }

    /// A job the process joins, owned by this value, whose close kills what is left in it.
    #[cfg(windows)]
    fn adopt(pid: u32, process: windows_sys::Win32::Foundation::HANDLE) -> io::Result<Tree> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation, SetInformationJobObject,
        };
        // SAFETY: plain kernel calls on handles this function owns; `limits` is the struct the
        // information class names, passed with its own size.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
            let info = (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast();
            if SetInformationJobObject(job, JobObjectExtendedLimitInformation, info, size) == 0
                || AssignProcessToJobObject(job, process) == 0
            {
                let err = io::Error::last_os_error();
                CloseHandle(job);
                return Err(err);
            }
            Ok(Tree { pid, job })
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Ask everything in the tree to leave, so it can save its state on the way out: `SIGTERM` to
    /// the group on unix; Ctrl+Break to the console group on Windows, which a console child with no
    /// handler leaves on, and one in a console of its own never hears — the liveness pipe, closed
    /// before this, is what reaches it.
    pub fn ask(&self) -> Result<(), String> {
        #[cfg(unix)]
        return signal(self.pid, libc::SIGTERM);
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Console::{GenerateConsoleCtrlEvent, CTRL_BREAK_EVENT};
            // SAFETY: a plain call naming the group the child leads.
            if unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, self.pid) } == 0 {
                return Err(io::Error::last_os_error().to_string());
            }
            Ok(())
        }
    }

    /// Insist, once the grace has run out.
    pub fn kill(&self) -> Result<(), String> {
        #[cfg(unix)]
        return signal(self.pid, libc::SIGKILL);
        #[cfg(windows)]
        {
            // SAFETY: a plain call on the job handle this value owns.
            if unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(self.job, 1) } == 0 {
                return Err(io::Error::last_os_error().to_string());
            }
            Ok(())
        }
    }
}

#[cfg(windows)]
impl Drop for Tree {
    /// Closing the job kills whatever is still in it.
    fn drop(&mut self) {
        // SAFETY: the handle this value owns, closed once.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.job) };
    }
}

/// Whether a process by that pid is still running — any process, not only a child of this one.
pub fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // SAFETY: signal 0 delivers nothing; it only asks whether the pid may be signalled.
        if unsafe { libc::kill(pid as i32, 0) } == 0 {
            return true;
        }
        io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
        use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        // SAFETY: a handle opened on a pid, queried and closed here.
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return false;
            }
            let mut code = 0u32;
            let asked = GetExitCodeProcess(process, &mut code) != 0;
            CloseHandle(process);
            asked && code == STILL_ACTIVE as u32
        }
    }
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

// ---- the liveness pipe ----

/// Create the liveness pipe, arrange for `cmd`'s child to inherit the read end, and name it in
/// [`LIVENESS_ENV`]. The returned read end must outlive `cmd.spawn()`.
fn arm(cmd: &mut Command) -> io::Result<(PipeWriter, PipeReader)> {
    let (reader, writer) = io::pipe()?;
    // Only the READ end is shared: std pipes are CLOEXEC, so the child's EOF means this
    // process died, not that a cousin still holds a write end.
    let value = share_read_end(cmd, &reader)?;
    cmd.env(LIVENESS_ENV, value);
    Ok((writer, reader))
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
