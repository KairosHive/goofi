//! goofi's one time module: the patch clock every stamp reads, and every way anything waits,
//! paces or measures against it. The event every parked thread wakes on, with the deadlines the
//! kernel keeps to the microsecond; the grids a periodic duty lands on; the deadline a bounded
//! wait counts down; the readable-wait a socket is parked on; the rate a stream of instants is
//! measured at. Nothing else in goofi sleeps in a loop, sets a socket timeout, rounds a deadline
//! to a tick or keeps a clock of its own.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, LazyLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::sync::Mutex;

// ---- the patch clock ----

/// The process base an origin is stored against, so the origin itself is one atomic — which is
/// what lets a render or callback thread read the time with no lock.
static BASE: LazyLock<Instant> = LazyLock::new(Instant::now);

/// What drives an engine with a clock of its own: the harness's hand, or the engine itself —
/// the device the audio nodes name, or the render timer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clock {
    External,
    Internal,
}

impl Clock {
    /// The word `session status` carries for it.
    pub fn name(self) -> &'static str {
        match self {
            Clock::External => "external",
            Clock::Internal => "internal",
        }
    }
}

fn since_base() -> u64 {
    BASE.elapsed().as_nanos() as u64
}

/// The patch's time. Every engine, every node and every stamp reads THIS object; nothing copies
/// what it says. A load restarts it, because a patch loaded an hour in begins at zero.
#[derive(Debug)]
pub struct Time {
    origin: AtomicU64,
    wall: Mutex<SystemTime>,
}

impl Default for Time {
    fn default() -> Time {
        Time::new()
    }
}

impl Time {
    pub fn new() -> Time {
        Time { origin: AtomicU64::new(since_base()), wall: Mutex::new(SystemTime::now()) }
    }

    /// Seconds since the patch began.
    pub fn now(&self) -> f64 {
        since_base().saturating_sub(self.origin.load(Ordering::Relaxed)) as f64 / 1e9
    }

    /// The instant a patch second falls on: what a wait toward a patch-time deadline parks until.
    pub fn instant_at(&self, seconds: f64) -> Instant {
        let origin = *BASE + Duration::from_nanos(self.origin.load(Ordering::Relaxed));
        origin + Duration::from_secs_f64(seconds.max(0.0))
    }

    /// The wall time the patch began at — the one UTC a recording's manifest states, so nothing
    /// downstream mints a second origin.
    pub fn wall(&self) -> SystemTime {
        *self.wall.lock()
    }

    /// The UTC of now, anchored ONCE at the origin and advanced by the monotonic clock. An NTP
    /// step moves `SystemTime::now()` and does not move this, which is what aligns two files.
    pub fn utc(&self) -> SystemTime {
        self.utc_at(self.now())
    }

    /// The UTC a patch second falls on.
    pub fn utc_at(&self, seconds: f64) -> SystemTime {
        self.wall() + Duration::from_secs_f64(seconds.max(0.0))
    }

    /// Begin again from now.
    pub fn restart(&self) {
        self.origin.store(since_base(), Ordering::Relaxed);
        *self.wall.lock() = SystemTime::now();
    }
}

/// UTC as `YYYYMMDD-HHMMSS`, plus the part of a second past that whole second.
fn civil(t: SystemTime) -> (String, Duration) {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let (secs, rest) = (d.as_secs(), d.subsec_nanos());
    let (days, day_secs) = ((secs / 86_400) as i64, secs % 86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let text = format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        day_secs / 3600,
        (day_secs / 60) % 60,
        day_secs % 60
    );
    (text, Duration::from_nanos(u64::from(rest)))
}

/// UTC as `YYYYMMDD-HHMMSS.mmm`, so a file sorts in the order it was written.
pub fn stamp(t: SystemTime) -> String {
    let (text, rest) = civil(t);
    format!("{text}.{:03}", rest.as_millis())
}

/// UTC as `YYYYMMDD-HHMMSS.nnnnnnnnn`, where a frame's own resolution is the point.
pub fn stamp_nanos(t: SystemTime) -> String {
    let (text, rest) = civil(t);
    format!("{text}.{:09}", rest.subsec_nanos())
}

// ---- deadlines ----

/// An instant a bounded wait counts down to: what is left of it is asked as often as the wait
/// has parts, and a part that comes after it gets nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deadline(Instant);

impl Deadline {
    pub fn within(d: Duration) -> Deadline {
        Deadline(Instant::now() + d)
    }

    pub fn at(instant: Instant) -> Deadline {
        Deadline(instant)
    }

    pub fn instant(self) -> Instant {
        self.0
    }

    /// What is left, zero once it passed.
    pub fn left(self) -> Duration {
        self.0.saturating_duration_since(Instant::now())
    }

    pub fn passed(self) -> bool {
        Instant::now() >= self.0
    }
}

// ---- events ----

/// How many ids an [`Event`] tells apart: `0..=255`.
pub const IDS: usize = 256;

/// The ids one wake found set, each once however often it was rung.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Woken([u64; IDS / 64]);

impl Woken {
    fn set(&mut self, id: u8) {
        self.0[usize::from(id) / 64] |= 1 << (u32::from(id) % 64);
    }

    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|w| *w == 0)
    }

    pub fn has(&self, id: u8) -> bool {
        self.0[usize::from(id) / 64] & (1 << (u32::from(id) % 64)) != 0
    }

    /// Every id set, ascending.
    pub fn ids(&self) -> impl Iterator<Item = u8> + '_ {
        self.0.iter().enumerate().flat_map(|(word, bits)| {
            (0..64u32).filter(move |bit| bits & (1 << bit) != 0).map(move |bit| (word as u32 * 64 + bit) as u8)
        })
    }
}

/// A door: rung with an id by anyone holding it, parked on by the one thread it belongs to. A
/// ring made while nobody waits is kept until the next wait; several rings of one id are one.
#[derive(Default)]
pub struct Event {
    pending: Mutex<Woken>,
    cv: Condvar,
}

impl Event {
    pub fn new() -> Event {
        Event::default()
    }

    /// Set `id` and wake whoever waits.
    pub fn ring(&self, id: u8) {
        self.pending.lock().set(id);
        self.cv.notify_all();
    }

    /// Whatever is set right now, taken; empty when nothing rang.
    pub fn take(&self) -> Woken {
        std::mem::take(&mut *self.pending.lock())
    }

    /// Park until something rings, and take it.
    pub fn wait(&self) -> Woken {
        let mut pending = self.pending.lock();
        while pending.is_empty() {
            pending = self.cv.wait(pending).unwrap_or_else(|e| e.into_inner());
        }
        std::mem::take(&mut *pending)
    }

    /// Park until something rings or `deadline` passes, and take what is set; empty on the
    /// deadline. A deadline already past looks once and returns.
    pub fn wait_until(&self, deadline: Instant) -> Woken {
        let mut pending = self.pending.lock();
        loop {
            if !pending.is_empty() {
                return std::mem::take(&mut *pending);
            }
            let now = Instant::now();
            if now >= deadline {
                return Woken::default();
            }
            pending = self.cv.wait_timeout(pending, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// Park for at most `within`.
    pub fn wait_for(&self, within: Duration) -> Woken {
        self.wait_until(Instant::now() + within)
    }

    /// Park until no ring has come for `quiet`, or `cap` passes: a debounce. Every id rung on
    /// the way is in the answer.
    pub fn wait_quiet(&self, quiet: Duration, cap: Deadline) -> Woken {
        let mut seen = Woken::default();
        loop {
            let rang = self.wait_until(cap.instant().min(Instant::now() + quiet));
            if rang.is_empty() || cap.passed() {
                return seen;
            }
            for id in rang.ids() {
                seen.set(id);
            }
        }
    }
}

/// A periodic duty's grid: deadlines `period` apart. A tick that overran restarts the grid from
/// now rather than firing a burst to catch up.
pub struct Pace {
    period: Duration,
    next: Instant,
}

impl Pace {
    pub fn new(period: Duration) -> Pace {
        Pace { period, next: Instant::now() + period }
    }

    pub fn period(&self) -> Duration {
        self.period
    }

    /// Change the period; the next deadline is a period from now.
    pub fn set_period(&mut self, period: Duration) {
        if period != self.period {
            self.period = period;
            self.next = Instant::now() + period;
        }
    }

    /// The deadline of the next tick.
    pub fn next(&self) -> Instant {
        self.next
    }

    /// Move the grid one tick on, and answer the deadline just set: the one to sleep until. A
    /// grid that fell behind the clock starts again from now.
    pub fn advance(&mut self) -> Instant {
        let now = Instant::now();
        self.next = if self.next + self.period < now { now + self.period } else { self.next + self.period };
        self.next
    }

    /// Start the grid again from now: the next deadline is a period from this instant, which a
    /// duty that was held off owes no burst of catch-up for.
    pub fn reset(&mut self) -> Instant {
        self.next = Instant::now() + self.period;
        self.next
    }

    /// Whether the next deadline has come.
    pub fn due(&self) -> bool {
        Instant::now() >= self.next
    }
}

/// A rate on ONE grid for the whole process: every duty at an interval lands on a tick of that
/// interval counted from one origin, so streams at one rate arrive together, a page paints them
/// once, and work never stretches the grid. Where [`Pace`] is a duty's own grid, this is shared.
pub struct Grid {
    tick: Instant,
    /// The interval `tick` is on; a tick armed on another interval is nobody's.
    interval: Duration,
    /// Whether something waits for `tick`: only then is a wake after it that tick's, late.
    armed: bool,
}

impl Default for Grid {
    fn default() -> Grid {
        Grid::new()
    }
}

/// The first tick of `interval`'s grid at or after `at`.
fn grid_tick(interval: Duration, at: Instant) -> Instant {
    let step = interval.as_nanos().max(1);
    let n = at.saturating_duration_since(*BASE).as_nanos().div_ceil(step);
    *BASE + Duration::from_nanos((n * step) as u64)
}

impl Grid {
    pub fn new() -> Grid {
        Grid { tick: Instant::now(), interval: Duration::ZERO, armed: false }
    }

    /// The tick to act on, armed by this ask. An armed one passed by more than half an interval
    /// is not a wake's lateness but an idle stretch, and the next tick takes its place.
    pub fn due(&mut self, interval: Duration, now: Instant) -> Instant {
        if !self.armed || self.interval != interval || self.tick + interval / 2 <= now {
            self.tick = grid_tick(interval, now);
            self.interval = interval;
            self.armed = true;
        }
        self.tick
    }

    /// A duty took this tick: the next one is nobody's until a wait arms it.
    pub fn take(&mut self, interval: Duration, now: Instant) {
        self.tick = grid_tick(interval, now + Duration::from_nanos(1));
        self.armed = false;
    }
}

// ---- measurement ----

/// The rate a stream of instants arrives at: the smoothed interval between them, inverted. A
/// stream of one has no rate yet; a clock that did not advance keeps the last estimate.
#[derive(Default, Debug, Clone, Copy)]
pub struct RateMeter {
    last: Option<f64>,
    ema: Option<f64>,
}

impl RateMeter {
    /// One arrival at `now` seconds; the rate in hertz once two have come, smoothed by `alpha`.
    pub fn note(&mut self, now: f64, alpha: f64) -> Option<f64> {
        let prev = self.last.replace(now)?;
        let dt = now - prev;
        if dt > 0.0 {
            self.ema = Some(self.ema.map_or(dt, |p| alpha * dt + (1.0 - alpha) * p));
        }
        self.ema.map(|e| 1.0 / e)
    }
}

// ---- sleeping and socket waits ----

/// Sleep until `deadline`, with the kernel's high-resolution timer; a deadline past returns.
pub fn sleep_until(deadline: Instant) {
    let now = Instant::now();
    if deadline > now {
        std::thread::sleep(deadline - now);
    }
}

/// Park until `socket` has bytes to read, or `deadline` passes; whether it is readable. The
/// socket's own timeout is never set: on Linux that is a tick-granular wait.
#[cfg(unix)]
pub fn wait_readable(socket: &impl std::os::fd::AsRawFd, deadline: Instant) -> std::io::Result<bool> {
    let mut fds = libc::pollfd { fd: socket.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        // `poll` takes whole milliseconds; rounded up, so a wait never ends early.
        let ms = left.as_millis().min(i32::MAX as u128) as i32 + i32::from(!left.subsec_nanos().is_multiple_of(1_000_000));
        // SAFETY: one pollfd, for the call's duration.
        let n = unsafe { libc::poll(&mut fds, 1, ms) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        return Ok(n > 0);
    }
}

/// As on Unix, through WinSock's `select`.
#[cfg(windows)]
pub fn wait_readable(socket: &impl std::os::windows::io::AsRawSocket, deadline: Instant) -> std::io::Result<bool> {
    use windows_sys::Win32::Networking::WinSock::{select, FD_SET, SOCKET_ERROR, TIMEVAL};
    let raw = socket.as_raw_socket() as usize;
    let left = deadline.saturating_duration_since(Instant::now());
    let micros = left.as_micros().min(i64::MAX as u128) as i64;
    let mut tv = TIMEVAL { tv_sec: (micros / 1_000_000) as i32, tv_usec: (micros % 1_000_000) as i32 };
    // SAFETY: an FD_SET holding one socket, and a timeval, for the call's duration.
    unsafe {
        let mut set: FD_SET = std::mem::zeroed();
        set.fd_count = 1;
        set.fd_array[0] = raw;
        let n = select(0, &mut set, std::ptr::null_mut(), std::ptr::null_mut(), &mut tv);
        if n == SOCKET_ERROR {
            return Err(std::io::Error::last_os_error());
        }
        Ok(n > 0)
    }
}
