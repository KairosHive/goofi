//! A spawned child's request/response pair over two byte streams and one bell. Neither side
//! polls: the child rings when its ports stand, and each side rings once it has written.

use std::time::{Duration, Instant};

use crate::services::{event_service, publish_with, publisher, stream_service, subscriber, write_parts, Doorbell, ServiceKind};
use crate::{BytePublisher, ByteSubscriber, Iox, Listener, PortBundle};

/// The largest frame an exchange carries in one sample; a publisher grows to it by powers of two.
pub const EXCHANGE_PAYLOAD: usize = 64 * 1024;

/// The bell's three rings: the child's ports stand, a request was written, a reply was written.
const READY: u8 = 0;
const ASKED: u8 = 1;
const ANSWERED: u8 = 2;

/// How long one wait on the bell lasts before the other side's liveness is looked at.
const SLICE: Duration = Duration::from_millis(100);

/// The parent's end: `[u32 seq][frame]` each way, the reply carrying the request's sequence. It
/// opens BEFORE the child starts, so the child's `READY` ring is never missed.
pub struct Exchange {
    ports: PortBundle<Pair<BytePublisher, ByteSubscriber>>,
    seq: u32,
    ready: bool,
}

/// The two ends of a request/response pair and the bell between them, whichever side holds them.
pub struct Pair<Q, A> {
    request: Q,
    reply: A,
    bell: Doorbell,
    listener: Listener,
}

/// The bell of the pair under `base`, on the node the ports are built on.
fn bell(node: &crate::IoxNode, base: &str) -> Result<(Doorbell, Listener), String> {
    let name = format!("{base}_bell");
    let bell = Doorbell::open(node, &name)?;
    let listener = event_service(node, &name)?.listener_builder().create().map_err(|e| format!("listener `{name}`: {e}"))?;
    Ok((bell, listener))
}

/// Wait on the bell for up to `within`, answering whether `id` rang.
fn rang(listener: &Listener, id: u8, within: Duration) -> bool {
    let mut heard = false;
    crate::wait_within(listener, within, |event| heard |= event.as_value() == id as usize);
    heard
}

/// A `[u32 seq][frame]` message, split.
fn split_seq(payload: &[u8]) -> Option<(u32, &[u8])> {
    let (seq, frame) = payload.split_first_chunk::<4>()?;
    Some((u32::from_le_bytes(*seq), frame))
}

impl Exchange {
    /// Open the pair under `base`: `<base>_req`, `<base>_resp` and `<base>_bell`, the names the
    /// child is told.
    pub fn open(iox: &Iox, base: &str) -> Result<Exchange, String> {
        let ports = PortBundle::open(iox, |node| {
            let request = publisher(&stream_service(node, &format!("{base}_req"), ServiceKind::Exchange)?, "request", EXCHANGE_PAYLOAD)?;
            let reply = subscriber(&stream_service(node, &format!("{base}_resp"), ServiceKind::Exchange)?, "reply")?;
            let (bell, listener) = bell(node, base)?;
            Ok(Pair { request, reply, bell, listener })
        })?;
        Ok(Exchange { ports, seq: 0, ready: false })
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
            loop {
                match self.ports.reply.receive() {
                    Ok(Some(sample)) => {
                        if let Some((_, frame)) = split_seq(sample.payload()).filter(|(s, _)| *s == seq) {
                            return Ok(frame.to_vec());
                        }
                    }
                    Ok(None) => break,
                    Err(e) => return Err(format!("iox receive: {e}")),
                }
            }
            // Checked AFTER draining, so a child that answered and then exited still gets its answer through.
            check(child, by)?;
            rang(&self.ports.listener, ANSWERED, SLICE);
        }
    }

    /// Write one request and ring; [`Self::answer`] waits for what comes back.
    pub fn send(&mut self, child: &mut goofi_supervisor::child::Child, frame: &[&[u8]]) -> Result<u32, String> {
        while !self.ready {
            self.ready = rang(&self.ports.listener, READY, SLICE);
            if !self.ready {
                check(child, None)?;
            }
        }
        self.seq = self.seq.wrapping_add(1);
        let seq = self.seq;
        while matches!(self.ports.reply.receive(), Ok(Some(_))) {}
        let seq_bytes = seq.to_le_bytes();
        let len = 4 + frame.iter().map(|p| p.len()).sum::<usize>();
        let parts = std::iter::once(&seq_bytes[..]).chain(frame.iter().copied());
        publish_with(&self.ports.request, len, |loan| write_parts(loan, parts), [(&self.ports.bell, ASKED)])?;
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
    ports: PortBundle<Pair<ByteSubscriber, BytePublisher>>,
    answered: Option<u32>,
}

impl Served {
    /// The base the parent gave as `GOOFI_IOX_BASE` in the environment.
    pub fn open_from_env() -> Result<Served, String> {
        let base = std::env::var("GOOFI_IOX_BASE").map_err(|_| "GOOFI_IOX_BASE is not set: not started by goofi")?;
        Served::open(&Iox::from_env()?, &base)
    }

    /// Open the child's end under `base` and ring `READY`: from here on a request reaches it.
    pub fn open(iox: &Iox, base: &str) -> Result<Served, String> {
        let ports = PortBundle::open(iox, |node| {
            let request = subscriber(&stream_service(node, &format!("{base}_req"), ServiceKind::Exchange)?, "request")?;
            let reply = publisher(&stream_service(node, &format!("{base}_resp"), ServiceKind::Exchange)?, "reply", EXCHANGE_PAYLOAD)?;
            let (bell, listener) = bell(node, base)?;
            Ok(Pair { request, reply, bell, listener })
        })?;
        ports.bell.ring(READY)?;
        Ok(Served { ports, answered: None })
    }

    /// Wait up to `within` for a request, then the latest one if a new one arrived: latest wins,
    /// and the one already answered is not a request.
    pub fn wait(&mut self, within: Duration) -> Result<Option<(u32, Vec<u8>)>, String> {
        rang(&self.ports.listener, ASKED, within);
        let mut latest = None;
        loop {
            match self.ports.request.receive() {
                Ok(Some(s)) => latest = Some(s),
                Ok(None) => break,
                Err(e) => return Err(format!("iox receive: {e}")),
            }
        }
        let Some(sample) = latest else { return Ok(None) };
        let fresh = split_seq(sample.payload()).filter(|(seq, _)| self.answered != Some(*seq));
        Ok(fresh.map(|(seq, frame)| (seq, frame.to_vec())))
    }

    pub fn answer(&mut self, seq: u32, reply: &[u8]) -> Result<(), String> {
        let seq_bytes = seq.to_le_bytes();
        publish_with(&self.ports.reply, 4 + reply.len(), |loan| write_parts(loan, [&seq_bytes[..], reply]), [(&self.ports.bell, ANSWERED)])?;
        self.answered = Some(seq);
        Ok(())
    }
}
