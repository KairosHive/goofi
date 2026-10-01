//! A spawned child's request/response pair over two byte streams and one bell, the parent's end
//! and the child's. Neither side polls: the child rings when its ports stand, and each side rings
//! once it has written.

use std::time::{Duration, Instant};

use iceoryx2::prelude::*;

use crate::services::{event_service, stream_service, write_parts, Doorbell, ServiceKind};
use crate::{BytePublisher, ByteSubscriber, Iox, Listener, PortBundle};

/// The largest frame an exchange carries in one sample; a publisher grows to it by powers of two.
pub const EXCHANGE_PAYLOAD: usize = 64 * 1024;

/// The bell's three rings: the child's ports stand, a request was written, a reply was written.
const READY: u8 = 0;
const ASKED: u8 = 1;
const ANSWERED: u8 = 2;

/// How long one wait on the bell lasts before the other side's liveness is looked at.
const SLICE: Duration = Duration::from_millis(100);

/// A spawned child's request/response pair, the parent's end: `[u32 seq][frame]` each way, and
/// the reply is the one carrying the same sequence. The parent opens its end BEFORE the child
/// starts, so the child's `READY` ring is never missed.
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
    let _ = listener.timed_wait_all(|event| heard |= event.as_value() == id as usize, within);
    heard
}

impl Exchange {
    /// Open the pair under `base`: `<base>_req`, `<base>_resp` and `<base>_bell`, the names the
    /// child is told.
    pub fn open(iox: &Iox, base: &str) -> Result<Exchange, String> {
        let ports = PortBundle::open(iox, |node| {
            let request = stream_service(node, &format!("{base}_req"), ServiceKind::Exchange)?
                .publisher_builder()
                .initial_max_slice_len(EXCHANGE_PAYLOAD)
                .allocation_strategy(AllocationStrategy::PowerOfTwo)
                .create()
                .map_err(|e| format!("request publisher: {e}"))?;
            let reply = stream_service(node, &format!("{base}_resp"), ServiceKind::Exchange)?
                .subscriber_builder()
                .create()
                .map_err(|e| format!("reply subscriber: {e}"))?;
            let (bell, listener) = bell(node, base)?;
            Ok(Pair { request, reply, bell, listener })
        })?;
        Ok(Exchange { ports, seq: 0, ready: false })
    }

    /// One request to `child` — its runs, written into the loan as one frame — answered within
    /// `timeout`; a child that exited or fell silent is the error, so the owner can start a
    /// fresh one. The first request waits for the child's `READY` before it is written.
    pub fn ask(&mut self, child: &mut goofi_supervisor::child::Child, frame: &[&[u8]], timeout: Duration) -> Result<Vec<u8>, String> {
        let deadline = Instant::now() + timeout;
        while !self.ready {
            self.ready = rang(&self.ports.listener, READY, SLICE);
            if !self.ready {
                self.check(child, deadline)?;
            }
        }
        self.seq = self.seq.wrapping_add(1);
        let seq = self.seq;
        while matches!(self.ports.reply.receive(), Ok(Some(_))) {}
        let seq_bytes = seq.to_le_bytes();
        let parts: Vec<&[u8]> = std::iter::once(&seq_bytes[..]).chain(frame.iter().copied()).collect();
        send_parts(&self.ports.request, &parts)?;
        let _ = self.ports.bell.ring(ASKED);
        loop {
            loop {
                match self.ports.reply.receive() {
                    Ok(Some(sample)) => {
                        let payload = sample.payload();
                        if payload.len() >= 4 && u32::from_le_bytes(payload[0..4].try_into().unwrap()) == seq {
                            return Ok(payload[4..].to_vec());
                        }
                    }
                    Ok(None) => break,
                    Err(e) => return Err(format!("iox receive: {e}")),
                }
            }
            // Checked AFTER draining, so a child that answered and then exited still gets its answer through.
            self.check(child, deadline)?;
            rang(&self.ports.listener, ANSWERED, SLICE);
        }
    }

    /// Whether the child is still there to answer, and the time still is.
    fn check(&self, child: &mut goofi_supervisor::child::Child, deadline: Instant) -> Result<(), String> {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("the child exited: {status}"));
        }
        if Instant::now() >= deadline {
            return Err("the child did not answer in time".into());
        }
        Ok(())
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
            let request = stream_service(node, &format!("{base}_req"), ServiceKind::Exchange)?
                .subscriber_builder()
                .create()
                .map_err(|e| format!("request subscriber: {e}"))?;
            let reply = stream_service(node, &format!("{base}_resp"), ServiceKind::Exchange)?
                .publisher_builder()
                .initial_max_slice_len(EXCHANGE_PAYLOAD)
                .allocation_strategy(AllocationStrategy::PowerOfTwo)
                .create()
                .map_err(|e| format!("reply publisher: {e}"))?;
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
        let payload = sample.payload();
        if payload.len() < 4 {
            return Ok(None);
        }
        let seq = u32::from_le_bytes(payload[0..4].try_into().unwrap());
        if self.answered == Some(seq) {
            return Ok(None);
        }
        Ok(Some((seq, payload[4..].to_vec())))
    }

    pub fn answer(&mut self, seq: u32, reply: &[u8]) -> Result<(), String> {
        send_parts(&self.ports.reply, &[&seq.to_le_bytes(), reply])?;
        self.answered = Some(seq);
        let _ = self.ports.bell.ring(ANSWERED);
        Ok(())
    }
}

/// One message of `parts` onto a service; the caller rings.
fn send_parts(publisher: &BytePublisher, parts: &[&[u8]]) -> Result<(), String> {
    let len = parts.iter().map(|p| p.len()).sum();
    let mut sample = publisher.loan_slice_uninit(len).map_err(|e| format!("iox loan: {e}"))?;
    write_parts(sample.payload_mut(), parts.iter().copied());
    // SAFETY: `write_parts` filled the loan exactly.
    unsafe { sample.assume_init() }.send().map_err(|e| format!("iox send: {e}"))?;
    Ok(())
}
