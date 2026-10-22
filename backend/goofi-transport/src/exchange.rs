//! A spawned child's request/response pair: a mailbox in shared memory each way, and a doorbell
//! between the two processes. Neither side polls: the child rings when it stands, and each side
//! rings once it has written.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::link::{self, Line};
use crate::shm::Segment;
use crate::Hub;

/// What a child reads to find its parent: `<session>;<exchange>;<port>;<token>`.
pub const ENV: &str = "GOOFI_EXCHANGE";

/// The bell's three rings: the child's end stands, a request was written, a reply was written.
const READY: u8 = 0;
const ASKED: u8 = 1;
const ANSWERED: u8 = 2;

/// How long one wait on the bell lasts before the other side's liveness is looked at.
pub(crate) const SLICE: Duration = Duration::from_millis(100);

/// What a mailbox's first segment holds; a larger message moves it to a larger one.
const FIRST_CAPACITY: usize = 64 * 1024;

/// A mailbox segment: `[capacity][seq][len][moved]` then the message. `seq` is odd while a
/// write is in progress; `moved` names the generation the mailbox went on to, or 0.
const HEADER: usize = 32;

/// The whole length of a segment holding `capacity` bytes of message.
pub(crate) fn mailbox_len(capacity: usize) -> usize {
    HEADER + capacity
}

/// The name of one mailbox segment, `/g<session>_<exchange><side><generation>`: under the
/// session's prefix, which the session sweep removes a crash's leftovers by, and short, for
/// macOS's 31-character limit on them.
fn segment_name(session: &str, exchange: u64, side: char, generation: u64) -> String {
    let prefix = if cfg!(windows) { "Local\\" } else { "/" };
    format!("{prefix}{}{exchange}{side}{generation}", goofi_supervisor::session::shm_prefix(session))
}

/// The header words of a mapped segment.
struct Head {
    capacity: *const AtomicU64,
    seq: *const AtomicU64,
    len: *const AtomicU64,
    moved: *const AtomicU64,
}

impl Head {
    fn of(segment: &Segment) -> Head {
        let base = segment.as_ptr() as *const AtomicU64;
        // SAFETY: the segment is page-aligned and at least HEADER bytes long.
        unsafe { Head { capacity: base, seq: base.add(1), len: base.add(2), moved: base.add(3) } }
    }
    fn capacity(&self) -> usize {
        // SAFETY: a word of a live mapping.
        unsafe { (*self.capacity).load(Ordering::Relaxed) as usize }
    }
}

/// One direction of an exchange: the newest message, seqlocked, in the segment of its current
/// generation. The writer grows it; the reader follows.
struct Mailbox {
    session: String,
    exchange: u64,
    side: char,
    generation: u64,
    current: Segment,
    /// What the reader took last, so a message is handed over once.
    taken: u64,
}

impl Mailbox {
    fn make(session: &str, exchange: u64, side: char, generation: u64, capacity: usize) -> Result<Mailbox, String> {
        let name = segment_name(session, exchange, side, generation);
        let current = Segment::create(&name, mailbox_len(capacity))?;
        // SAFETY: a fresh, zeroed mapping; the capacity is what `open` reads first.
        unsafe { (*Head::of(&current).capacity).store(capacity as u64, Ordering::Release) };
        Ok(Mailbox { session: session.to_string(), exchange, side, generation, current, taken: 0 })
    }

    fn open(session: &str, exchange: u64, side: char, generation: u64) -> Result<Mailbox, String> {
        let name = segment_name(session, exchange, side, generation);
        let current = Segment::open(&name)?;
        if current.len() < HEADER {
            return Err(format!("the segment {name} is too short"));
        }
        Ok(Mailbox { session: session.to_string(), exchange, side, generation, current, taken: 0 })
    }

    /// Write one message, `parts` end to end, and bump the sequence. A message the segment
    /// cannot hold goes into a larger one, and the old segment says where.
    fn write(&mut self, parts: &[&[u8]]) -> Result<(), String> {
        let len: usize = parts.iter().map(|p| p.len()).sum();
        let head = Head::of(&self.current);
        if len > head.capacity() {
            let bigger = len.next_power_of_two().max(head.capacity() * 2);
            let next = Mailbox::make(&self.session, self.exchange, self.side, self.generation + 1, bigger)?;
            // SAFETY: header words of a live mapping; the move is published after the sequence,
            // so a reader that saw the sequence move finds the forward.
            unsafe {
                (*head.moved).store(next.generation, Ordering::Release);
                (*head.seq).fetch_add(2, Ordering::AcqRel);
            }
            self.current = next.current;
            self.generation = next.generation;
            return self.write(parts);
        }
        // SAFETY: the header and the message area of a mapping sized by `capacity`; the odd
        // sequence tells a concurrent reader the bytes are in flight.
        unsafe {
            (*head.seq).fetch_add(1, Ordering::AcqRel);
            let mut at = self.current.as_ptr().add(HEADER);
            for part in parts {
                std::ptr::copy_nonoverlapping(part.as_ptr(), at, part.len());
                at = at.add(part.len());
            }
            (*head.len).store(len as u64, Ordering::Release);
            (*head.seq).fetch_add(1, Ordering::AcqRel);
        }
        Ok(())
    }

    /// The newest message not yet taken, copied out whole; `None` when nothing new is there.
    fn read(&mut self) -> Result<Option<Vec<u8>>, String> {
        loop {
            let head = Head::of(&self.current);
            // SAFETY: header words of a live mapping.
            let moved = unsafe { (*head.moved).load(Ordering::Acquire) };
            if moved != 0 {
                let next = Mailbox::open(&self.session, self.exchange, self.side, moved)?;
                self.current = next.current;
                self.generation = moved;
                // The new segment counts from zero; what was taken from the old one is no guide.
                self.taken = 0;
                continue;
            }
            // SAFETY: as above.
            let seq = unsafe { (*head.seq).load(Ordering::Acquire) };
            if seq == self.taken || seq == 0 {
                return Ok(None);
            }
            if seq & 1 == 1 {
                std::thread::yield_now();
                continue;
            }
            // SAFETY: the message area, read under the sequence and checked against it after.
            let copied = unsafe {
                let len = (*head.len).load(Ordering::Acquire) as usize;
                if len > head.capacity() {
                    return Err("a mailbox message longer than its segment".into());
                }
                std::slice::from_raw_parts(self.current.as_ptr().add(HEADER), len).to_vec()
            };
            // The copy must be complete before the sequence is checked again; an acquire load
            // alone orders only what follows it.
            std::sync::atomic::fence(Ordering::Acquire);
            // SAFETY: as above.
            let again = unsafe { (*head.seq).load(Ordering::Acquire) };
            if again != seq {
                continue;
            }
            self.taken = seq;
            return Ok(Some(copied));
        }
    }
}

/// A `[u32 seq][frame]` message, split.
fn split_seq(payload: &[u8]) -> Option<(u32, &[u8])> {
    let (seq, frame) = payload.split_first_chunk::<4>()?;
    Some((u32::from_le_bytes(*seq), frame))
}

/// The parent's end: `[u32 seq][frame]` each way, the reply carrying the request's sequence. It
/// opens BEFORE the child starts, so the child's `READY` ring is never missed.
pub struct Exchange {
    request: Mailbox,
    reply: Mailbox,
    host: link::Host,
    line: Option<Line>,
    env: String,
    seq: u32,
}

impl Exchange {
    /// Open a fresh pair under `hub`'s session; [`Exchange::env`] is what the child is told.
    pub fn open(hub: &Hub) -> Result<Exchange, String> {
        let exchange = hub.next_exchange();
        let request = Mailbox::make(hub.id(), exchange, 'q', 0, FIRST_CAPACITY)?;
        let reply = Mailbox::make(hub.id(), exchange, 'a', 0, FIRST_CAPACITY)?;
        let host = link::Host::open()?;
        let env = format!("{};{exchange};{};{}", hub.id(), host.port(), host.token());
        Ok(Exchange { request, reply, host, line: None, env, seq: 0 })
    }

    /// The value of [`ENV`] the child reads.
    pub fn env(&self) -> &str {
        &self.env
    }

    /// One request to `child` and its answer. The wait ends only when the child answers, exits, or
    /// the thread's halt is raised: a slow child is not an error.
    pub fn ask(&mut self, child: &mut goofi_supervisor::child::Child, frame: &[&[u8]]) -> Result<Vec<u8>, String> {
        let seq = self.send(child, frame)?;
        self.answer(child, seq, None)
    }

    /// The answer to request `seq`. With `by`, the wait is a stop's and ends at that deadline;
    /// without it, at the thread's halt.
    pub fn answer(&mut self, child: &mut goofi_supervisor::child::Child, seq: u32, by: Option<Instant>) -> Result<Vec<u8>, String> {
        loop {
            if let Some(message) = self.reply.read()? {
                if let Some((_, frame)) = split_seq(&message).filter(|(s, _)| *s == seq) {
                    return Ok(frame.to_vec());
                }
            }
            // Checked AFTER reading, so a child that answered and then exited still gets its answer through.
            check(child, by)?;
            let line = self.line.as_mut().ok_or("the child's doorbell is not connected")?;
            let _ = line.wait_until(Instant::now() + SLICE).map_err(|e| format!("the doorbell: {e}"))?;
        }
    }

    /// Write one request and ring; [`Self::answer`] waits for what comes back.
    pub fn send(&mut self, child: &mut goofi_supervisor::child::Child, frame: &[&[u8]]) -> Result<u32, String> {
        while self.line.is_none() {
            match self.host.accept_until(Instant::now() + SLICE)? {
                Some(mut line) => {
                    // Its READY may already be in flight; what matters is that the end stands.
                    let _ = line.wait_until(Instant::now());
                    self.line = Some(line);
                }
                None => check(child, None)?,
            }
        }
        self.seq = self.seq.wrapping_add(1);
        let seq = self.seq;
        let seq_bytes = seq.to_le_bytes();
        let parts: Vec<&[u8]> = std::iter::once(&seq_bytes[..]).chain(frame.iter().copied()).collect();
        self.request.write(&parts)?;
        self.line.as_mut().expect("connected above").ring(ASKED).map_err(|e| format!("the doorbell: {e}"))?;
        Ok(seq)
    }
}

/// Whether the child is still there to answer, and the wait still runs: to `by` when given,
/// else until this thread's halt.
fn check(child: &mut goofi_supervisor::child::Child, by: Option<Instant>) -> Result<(), String> {
    if let Ok(Some(status)) = child.try_wait() {
        return Err(format!("the child exited: {status}"));
    }
    match by {
        Some(by) if Instant::now() >= by => Err("the child did not answer its stop".into()),
        None if crate::Halt::worn_stopped() => Err("the node is stopping".into()),
        _ => Ok(()),
    }
}

/// The child's end of an [`Exchange`]: the newest request not yet answered, and its answer.
pub struct Served {
    request: Mailbox,
    reply: Mailbox,
    line: Line,
    answered: Option<u32>,
}

impl Served {
    /// The pair [`ENV`] names, connected and announced `READY`: from here on a request reaches it.
    pub fn open_from_env() -> Result<Served, String> {
        let value = std::env::var(ENV).map_err(|_| format!("{ENV} is not set: not started by goofi"))?;
        let parts: Vec<&str> = value.split(';').collect();
        let [session, exchange, port, token] = parts.as_slice() else {
            return Err(format!("{ENV} is not `session;exchange;port;token`"));
        };
        let exchange: u64 = exchange.parse().map_err(|_| format!("{ENV}: a bad exchange number"))?;
        let port: u16 = port.parse().map_err(|_| format!("{ENV}: a bad port"))?;
        let request = Mailbox::open(session, exchange, 'q', 0)?;
        let reply = Mailbox::open(session, exchange, 'a', 0)?;
        let mut line = link::connect(port, token)?;
        line.ring(READY).map_err(|e| format!("the doorbell: {e}"))?;
        Ok(Served { request, reply, line, answered: None })
    }

    /// Wait up to `within` for a request, then the newest one: latest wins, and the one already
    /// answered is not a request.
    pub fn wait(&mut self, within: Duration) -> Result<Option<(u32, Vec<u8>)>, String> {
        let deadline = Instant::now() + within;
        loop {
            if let Some(message) = self.request.read()? {
                if let Some((seq, frame)) = split_seq(&message).filter(|(seq, _)| self.answered != Some(*seq)) {
                    return Ok(Some((seq, frame.to_vec())));
                }
            }
            let rang = self.line.wait_until(deadline).map_err(|e| format!("the doorbell: {e}"))?;
            if rang.is_empty() {
                return Ok(None);
            }
        }
    }

    pub fn answer(&mut self, seq: u32, reply: &[u8]) -> Result<(), String> {
        let seq_bytes = seq.to_le_bytes();
        self.reply.write(&[&seq_bytes[..], reply])?;
        self.answered = Some(seq);
        self.line.ring(ANSWERED).map_err(|e| format!("the doorbell: {e}"))
    }
}
