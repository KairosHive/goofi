//! Waiting and pacing, in one place: the event every parked thread of goofi wakes on, with the
//! deadlines the kernel keeps to the microsecond; the grid a periodic duty lands on; and the
//! readable-wait a socket is parked on. Nothing else in goofi sleeps in a loop, sets a socket
//! timeout or rounds a deadline to a tick.

use std::sync::Condvar;
use std::time::{Duration, Instant};

use crate::sync::Mutex;

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
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_coalesce_and_each_wakes_once() {
        let event = Event::new();
        event.ring(3);
        event.ring(3);
        event.ring(200);
        let woken = event.wait();
        assert_eq!(woken.ids().collect::<Vec<_>>(), vec![3, 200]);
        assert!(event.take().is_empty());
    }

    #[test]
    fn a_deadline_wait_returns_empty_on_time() {
        let event = Event::new();
        let t = Instant::now();
        let woken = event.wait_for(Duration::from_millis(5));
        assert!(woken.is_empty());
        assert!(t.elapsed() >= Duration::from_millis(5));
    }

    #[test]
    fn a_pace_that_fell_behind_restarts_from_now() {
        let mut pace = Pace::new(Duration::from_millis(10));
        let first = pace.next();
        std::thread::sleep(Duration::from_millis(30));
        let next = pace.advance();
        assert!(next > first + Duration::from_millis(10), "not a burst of catch-up ticks");
        assert!(next <= Instant::now() + Duration::from_millis(10));
    }
}
