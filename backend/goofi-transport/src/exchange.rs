//! A spawned child's request/response pair over two byte streams, the parent's end and the child's.

use std::time::{Duration, Instant};

use iceoryx2::prelude::*;

use crate::services::{stream_service, write_parts, ServiceKind};
use crate::{BytePublisher, ByteSubscriber, Iox, PortBundle};

/// The largest frame an exchange carries in one sample; a publisher grows to it by powers of two.
pub const EXCHANGE_PAYLOAD: usize = 64 * 1024;

/// A spawned child's request/response pair, the parent's end: `[u32 seq][frame]` each way. A
/// request is re-published each idle millisecond, since the child's subscriber may still be
/// connecting; the reply is the one carrying the same sequence.
pub struct Exchange {
    ports: PortBundle<Pair<BytePublisher, ByteSubscriber>>,
    seq: u32,
}

/// The two ends of a request/response pair, whichever side holds them.
pub struct Pair<Q, A> {
    request: Q,
    reply: A,
}

impl Exchange {
    /// Open the pair under `base`: `<base>_req` and `<base>_resp`, the names the child is told.
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
            Ok(Pair { request, reply })
        })?;
        Ok(Exchange { ports, seq: 0 })
    }

    /// One request to `child` — its runs, written into the loan as one frame — answered within
    /// `timeout`; a child that exited or fell silent is the error, so the owner can start a
    /// fresh one.
    pub fn ask(&mut self, child: &mut goofi_supervisor::child::Child, frame: &[&[u8]], timeout: Duration) -> Result<Vec<u8>, String> {
        self.seq = self.seq.wrapping_add(1);
        let seq = self.seq;
        while matches!(self.ports.reply.receive(), Ok(Some(_))) {}
        let seq_bytes = seq.to_le_bytes();
        let parts: Vec<&[u8]> = std::iter::once(&seq_bytes[..]).chain(frame.iter().copied()).collect();
        let deadline = Instant::now() + timeout;
        loop {
            send_parts(&self.ports.request, &parts)?;
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
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!("the child exited: {status}"));
            }
            if Instant::now() >= deadline {
                return Err("the child did not answer in time".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

/// The child's end of an [`Exchange`]: the newest request not yet answered, and its answer.
pub struct Served {
    ports: PortBundle<Pair<ByteSubscriber, BytePublisher>>,
    answered: Option<u32>,
}

impl Served {
    /// The names the parent gave: `GOOFI_IOX_REQ` and `GOOFI_IOX_RESP` in the environment.
    pub fn open_from_env() -> Result<Served, String> {
        let name = |key: &str| std::env::var(key).map_err(|_| format!("{key} is not set: not started by goofi"));
        Served::open(&Iox::from_env()?, &name("GOOFI_IOX_REQ")?, &name("GOOFI_IOX_RESP")?)
    }

    pub fn open(iox: &Iox, request: &str, reply: &str) -> Result<Served, String> {
        let ports = PortBundle::open(iox, |node| {
            let request = stream_service(node, request, ServiceKind::Exchange)?
                .subscriber_builder()
                .create()
                .map_err(|e| format!("request subscriber: {e}"))?;
            let reply = stream_service(node, reply, ServiceKind::Exchange)?
                .publisher_builder()
                .initial_max_slice_len(EXCHANGE_PAYLOAD)
                .allocation_strategy(AllocationStrategy::PowerOfTwo)
                .create()
                .map_err(|e| format!("reply publisher: {e}"))?;
            Ok(Pair { request, reply })
        })?;
        Ok(Served { ports, answered: None })
    }

    /// The latest request, if a new one arrived: latest wins, and a re-publish of the one already
    /// answered is not a request.
    pub fn request(&mut self) -> Result<Option<(u32, Vec<u8>)>, String> {
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
        Ok(())
    }
}

/// One message of `parts` onto a service with no bells: the exchange's two directions.
fn send_parts(publisher: &BytePublisher, parts: &[&[u8]]) -> Result<(), String> {
    let len = parts.iter().map(|p| p.len()).sum();
    let mut sample = publisher.loan_slice_uninit(len).map_err(|e| format!("iox loan: {e}"))?;
    write_parts(sample.payload_mut(), parts.iter().copied());
    // SAFETY: `write_parts` filled the loan exactly.
    unsafe { sample.assume_init() }.send().map_err(|e| format!("iox send: {e}"))?;
    Ok(())
}
