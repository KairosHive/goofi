//! The browser device, heard: the mix its clock renders goes out on `/audio` to every browser
//! that asks, as Opus packets or as 16-bit PCM. One worker frames and encodes it for all of them.

use std::sync::mpsc::Receiver;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket};
use goofi_audio::{Chunk, Outlet, BROWSER_CHANNELS, BROWSER_RATE};
use goofi_supervisor::sync::Mutex;
use serde_json::json;
use tokio::sync::broadcast;

/// The frames in one packet: 20 ms, an Opus frame.
pub const FRAME: usize = 960;

/// The packets a slow socket may fall behind before it loses the oldest: one second.
const BEHIND: usize = 50;

/// Opus at a rate where music is transparent to most ears.
const BITRATE: i32 = 128_000;

/// The largest packet the encoder may write: far above what `BITRATE` gives for one frame.
const PACKET: usize = 4000;

const CHANNELS: usize = BROWSER_CHANNELS as usize;

/// How a listener takes the mix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Opus,
    Pcm,
}

impl Codec {
    /// The codec a `/audio?codec=` query names; Opus where it names none.
    pub fn named(name: Option<&str>) -> Option<Codec> {
        match name {
            None | Some("opus") => Some(Codec::Opus),
            Some("pcm") => Some(Codec::Pcm),
            Some(_) => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Codec::Opus => "opus",
            Codec::Pcm => "pcm",
        }
    }
}

/// The stream to the browsers: a packet is the index of its first frame, 8 bytes little-endian,
/// and then one frame's payload in the codec of its channel.
#[derive(Clone)]
pub struct Listen {
    outlet: Arc<Outlet>,
    opus: broadcast::Sender<Bytes>,
    pcm: broadcast::Sender<Bytes>,
    /// How many sockets listen; the outlet is told when the first comes and the last goes.
    listeners: Arc<Mutex<usize>>,
}

/// One socket's place among the listeners, given back when it goes.
struct Here(Listen);

impl Drop for Here {
    fn drop(&mut self) {
        let mut n = self.0.listeners.lock();
        *n -= 1;
        if *n == 0 {
            self.0.outlet.listen(false);
        }
    }
}

impl Listen {
    pub fn new(outlet: Arc<Outlet>) -> Listen {
        Listen { outlet, opus: broadcast::channel(BEHIND).0, pcm: broadcast::channel(BEHIND).0, listeners: Default::default() }
    }

    /// Frame and encode the outlet's periods until it closes. The manager's worker runs this.
    pub fn serve(&self) {
        let Some(periods) = self.outlet.take() else { return };
        self.frames(periods);
    }

    /// Let the worker go: the outlet gives no more periods.
    pub fn close(&self) {
        self.outlet.close();
    }

    fn join(&self) -> Here {
        let mut n = self.listeners.lock();
        *n += 1;
        self.outlet.listen(true);
        Here(self.clone())
    }

    fn frames(&self, periods: Receiver<Chunk>) {
        let mut encoder = Encoder::new();
        // What is held of a frame not yet whole, and the index of its first frame.
        let (mut held, mut at): (Vec<f32>, u64) = (Vec::new(), 0);
        while let Ok(chunk) = periods.recv() {
            // A period lost on the way, or a clock opened again: the frame begins at this one.
            if chunk.first != at + (held.len() / CHANNELS) as u64 {
                held.clear();
                at = chunk.first;
            }
            held.extend_from_slice(&chunk.samples);
            while held.len() >= FRAME * CHANNELS {
                let frame: Vec<f32> = held.drain(..FRAME * CHANNELS).collect();
                if self.pcm.receiver_count() > 0 {
                    let words = frame.iter().flat_map(|v| ((v.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16).to_le_bytes());
                    let _ = self.pcm.send(packet(at, words));
                }
                if self.opus.receiver_count() > 0 {
                    if let Some(bytes) = encoder.as_mut().and_then(|e| e.encode(&frame)) {
                        let _ = self.opus.send(packet(at, bytes.iter().copied()));
                    }
                }
                at += FRAME as u64;
            }
        }
    }

    /// One browser's socket: what the stream is, in one text message, then its packets until
    /// either end goes. A socket that falls behind loses the oldest packets and plays on.
    pub async fn handle(self, mut socket: WebSocket, codec: Codec) {
        let mut packets = match codec {
            Codec::Opus => self.opus.subscribe(),
            Codec::Pcm => self.pcm.subscribe(),
        };
        let _here = self.join();
        let head = json!({ "codec": codec.name(), "rate": BROWSER_RATE, "channels": BROWSER_CHANNELS, "frame": FRAME });
        if socket.send(Message::Text(head.to_string().into())).await.is_err() {
            return;
        }
        loop {
            tokio::select! {
                packet = packets.recv() => match packet {
                    Ok(packet) => {
                        if socket.send(Message::Binary(packet)).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                said = socket.recv() => {
                    if !matches!(said, Some(Ok(Message::Text(_) | Message::Binary(_) | Message::Ping(_) | Message::Pong(_)))) {
                        break;
                    }
                }
            }
        }
    }
}

fn packet(first: u64, payload: impl Iterator<Item = u8>) -> Bytes {
    first.to_le_bytes().into_iter().chain(payload).collect::<Vec<u8>>().into()
}

/// libopus, one state for the one stream.
struct Encoder {
    state: *mut unsafe_libopus::OpusEncoder,
    out: Vec<u8>,
}

// SAFETY: the state is a heap block with no thread affinity, owned by this value alone.
unsafe impl Send for Encoder {}

impl Encoder {
    /// An encoder for the browser device's rate and width; none where libopus refuses, which is
    /// logged, and the PCM channel still serves.
    fn new() -> Option<Encoder> {
        let mut error = 0;
        // SAFETY: plain arguments, and `error` outlives the call.
        let state = unsafe { unsafe_libopus::opus_encoder_create(BROWSER_RATE as i32, CHANNELS as i32, unsafe_libopus::OPUS_APPLICATION_AUDIO, &mut error) };
        if state.is_null() || error != unsafe_libopus::OPUS_OK {
            goofi_supervisor::log::record(goofi_supervisor::log::Source::component("bridge"), goofi_supervisor::log::Level::Error, None, format!("audio: the Opus encoder did not open ({error})"));
            return None;
        }
        // SAFETY: `state` is the live encoder made above.
        unsafe { unsafe_libopus::opus_encoder_ctl!(state, unsafe_libopus::OPUS_SET_BITRATE_REQUEST, BITRATE) };
        Some(Encoder { state, out: vec![0; PACKET] })
    }

    /// One frame, interleaved, as one packet.
    fn encode(&mut self, frame: &[f32]) -> Option<&[u8]> {
        debug_assert_eq!(frame.len(), FRAME * CHANNELS);
        // SAFETY: `frame` holds FRAME frames of CHANNELS samples, and `out` is PACKET bytes.
        let n = unsafe { unsafe_libopus::opus_encode_float(self.state, frame.as_ptr(), FRAME as i32, self.out.as_mut_ptr(), PACKET as i32) };
        usize::try_from(n).ok().map(|n| &self.out[..n])
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: the state this value made, destroyed once.
        unsafe { unsafe_libopus::opus_encoder_destroy(self.state) };
    }
}
