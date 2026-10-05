//! The patch's time: one origin, read as seconds since the patch began.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The process base an origin is stored against, so the origin itself is one atomic — which is
/// what lets a render or callback thread read the time with no lock.
static BASE: LazyLock<Instant> = LazyLock::new(Instant::now);

/// A patch-clock instant, in fixed-point seconds. The fractional precision keeps a
/// sub-sample period accurate when deadlines are added over a long run.
pub type Tick = u128;
pub const TICKS_PER_SECOND: Tick = 1 << 64;
/// The shared observation grid for time-dependent control predicates.
pub const CONTROL_RATE: Tick = 48_000;

pub fn next_control_tick(at: Tick) -> Tick {
    let whole = at / TICKS_PER_SECOND * TICKS_PER_SECOND;
    let fraction = at % TICKS_PER_SECOND;
    let mut index = fraction * CONTROL_RATE / TICKS_PER_SECOND + 1;
    if index * TICKS_PER_SECOND / CONTROL_RATE <= fraction { index += 1; }
    whole + index * TICKS_PER_SECOND / CONTROL_RATE
}

pub fn ticks(seconds: f64) -> Option<Tick> {
    (seconds.is_finite() && seconds >= 0.0 && seconds < u64::MAX as f64)
        .then(|| (seconds * TICKS_PER_SECOND as f64).round() as Tick)
}

pub fn seconds(tick: Tick) -> f64 {
    tick as f64 / TICKS_PER_SECOND as f64
}

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
    wall: goofi_supervisor::sync::Mutex<SystemTime>,
}

impl Default for Time {
    fn default() -> Time {
        Time::new()
    }
}

impl Time {
    pub fn new() -> Time {
        Time { origin: AtomicU64::new(since_base()), wall: goofi_supervisor::sync::Mutex::new(SystemTime::now()) }
    }

    /// Seconds since the patch began.
    pub fn now(&self) -> f64 {
        self.now_nanos() as f64 / 1e9
    }

    /// Integer patch time for deadlines that must not accumulate rounding or wake delays.
    pub fn now_nanos(&self) -> u64 {
        since_base().saturating_sub(self.origin.load(Ordering::Relaxed))
    }

    pub fn now_ticks(&self) -> Tick {
        self.stamp().1
    }

    /// Origin identity and instant from one clock read. A load changes the identity.
    pub fn stamp(&self) -> (u64, Tick) {
        let origin = self.origin.load(Ordering::Relaxed);
        let nanos = since_base().saturating_sub(origin);
        (origin, u128::from(nanos) * TICKS_PER_SECOND / 1_000_000_000)
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
