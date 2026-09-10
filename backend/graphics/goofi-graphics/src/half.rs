//! The graphics engine's half of a node's control thread: an arrival becomes texels the render
//! thread uploads, and the frame that thread read back goes out while anyone drinks from it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use goofi_control::{Cx, Half, Ticked};
use goofi_core::Data;
pub use crate::resources::Upload;

/// One frame off the GPU, in the width its readers DRAW. The engine converts on the device, so
/// neither arm costs the CPU a conversion — and the 8-bit arm never becomes a `Data`, because
/// `Data` is f32 and expanding texels only to quantize them again downstream is the work this
/// exists to remove.
pub enum Tapped {
    Full(Data),
    Texels { shape: Vec<usize>, bytes: Vec<u8>, meta: goofi_core::Meta },
}

/// Where the render thread leaves the frame it read back, until the half takes it. Latest wins,
/// as every crossing into a scheduled engine is: what paces the readback is the ring's ONE slot in
/// flight, so an accessory can never pace the engine and never has to be waited for. A re-arm flag
/// beside it was a second owner of that pacing, and it cost a whole tick per frame — the half runs
/// on another thread, so the tick that took a frame could never see it set again.
#[derive(Default)]
pub struct Tap {
    pub frame: Option<Tapped>,
}

pub struct GraphicsHalf {
    producer: Option<crate::producer::Worker>,
    uploads: Vec<Arc<Mutex<Option<Upload>>>>,
    readers: Arc<AtomicBool>,
    tap: Arc<Mutex<Tap>>,
    /// Where the universal `common` size starts in the param atomics.
    size: usize,
    /// The size last seen there. Only a settle can re-plan a stage's target, so the half — which
    /// ticks beside the one writer of those atomics — is what asks for one when the size moves.
    last: (u32, u32),
}

impl GraphicsHalf {
    pub fn with_producer(mut self, producer: Option<crate::producer::Worker>) -> Self {
        self.producer = producer;
        self
    }

    pub fn new(
        uploads: Vec<Arc<Mutex<Option<Upload>>>>,
        readers: Arc<AtomicBool>,
        tap: Arc<Mutex<Tap>>,
        size: usize,
    ) -> GraphicsHalf {
        GraphicsHalf { producer: None, uploads, readers, tap, size, last: (u32::MAX, u32::MAX) }
    }
}

impl Half for GraphicsHalf {
    fn latest_only(&self) -> bool {
        true
    }

    /// An arrival replaces whatever the render thread has not taken yet: latest wins, as every
    /// crossing into a scheduled engine is.
    fn arrive(&mut self, inbox: usize, frame: &Data) -> bool {
        if let Some(producer) = &self.producer { producer.input(inbox, frame.clone()); return false; }
        let Some(cell) = self.uploads.get(inbox) else { return false };
        if let Some(up) = Upload::of(frame) {
            *cell.lock().unwrap() = Some(up);
        }
        false
    }

    fn unwired(&mut self, inbox: usize) {
        if let Some(producer) = &self.producer { producer.unwired(inbox); }
    }
    fn refresh(&mut self) -> Option<Vec<String>> {
        if let Some(producer) = &self.producer { producer.refresh(); }
        None
    }
    fn tick(&mut self, cx: &Cx<'_>, publish: &mut dyn FnMut(usize, &[u8])) -> Ticked {
        if let Some(producer) = &mut self.producer { producer.sync(cx); }
        // What the render thread reads to decide whether this node runs at all.
        let readers = cx.readers.first().copied().unwrap_or(false);
        self.readers.store(readers, Ordering::Relaxed);
        // Taken from UNDER the lock and encoded outside it: the render thread waits on this
        // mutex, so an encode held across it is the frontend stalling a node tick.
        let taken = self.tap.lock().expect("the tap").frame.take();
        match taken {
            Some(Tapped::Full(frame)) => publish(0, &goofi_codec::encode(&frame)),
            Some(Tapped::Texels { shape, bytes, meta }) => {
                publish(0, &goofi_codec::encode_u8(&shape, &bytes, &meta))
            }
            None => {}
        }
        let size = crate::plan::asked(cx.params, self.size);
        Ticked { errors: Vec::new(), replan: std::mem::replace(&mut self.last, size) != size }
    }
}
