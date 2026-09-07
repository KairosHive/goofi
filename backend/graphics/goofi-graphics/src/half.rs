//! The graphics engine's half of a node's control thread: an arrival becomes texels the render
//! thread uploads, and the frame that thread read back goes out while anyone drinks from it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use goofi_control::{Cx, Half, Ticked};
use goofi_core::Data;

use crate::transfer::{self, Range, Upload, PER_INPUT};

/// What the render thread and the control half hand each other: the frame one read back, and
/// whether the other is ready for the next. The engine reads back for a viewer only when the
/// viewer has FINISHED with the last frame, so an accessory can never pace the engine — and
/// never has to be waited for either.
#[derive(Default)]
pub struct Tap {
    /// The render thread's: the frame it left, until the half takes it.
    pub frame: Option<Data>,
    /// The half's: it has published what it had and will take another.
    pub wanted: bool,
}

/// One ARRAY input between two ticks: the frame that last arrived, what the texels in its cell
/// were made under, and the range a trajectory carries across frames.
#[derive(Default)]
struct Inbox {
    frame: Option<Data>,
    fresh: bool,
    made: Option<([u64; PER_INPUT], (u32, u32))>,
    range: Range,
}

pub struct GraphicsHalf {
    uploads: Vec<Arc<Mutex<Option<Upload>>>>,
    inboxes: Vec<Inbox>,
    readers: Arc<AtomicBool>,
    tap: Arc<Mutex<Tap>>,
    /// Where the first ARRAY input's transfer params start.
    transfer: usize,
    /// Where the universal `common` size starts in the param atomics.
    size: usize,
    /// The size last seen there. Only a settle can re-plan a stage's target, so the half — which
    /// ticks beside the one writer of those atomics — is what asks for one when the size moves.
    last: (u32, u32),
}

impl GraphicsHalf {
    pub fn new(
        uploads: Vec<Arc<Mutex<Option<Upload>>>>,
        readers: Arc<AtomicBool>,
        tap: Arc<Mutex<Tap>>,
        transfer: usize,
        size: usize,
    ) -> GraphicsHalf {
        let inboxes = uploads.iter().map(|_| Inbox::default()).collect();
        GraphicsHalf { uploads, inboxes, readers, tap, transfer, size, last: (u32::MAX, u32::MAX) }
    }

    /// Draw what each input holds, where this tick is the first to see that frame under those
    /// params — a batch of arrivals between two ticks costs one drawing, and none at all costs
    /// nothing.
    fn transfer(&mut self, cx: &Cx<'_>, at: (u32, u32)) {
        for (k, inbox) in self.inboxes.iter_mut().enumerate() {
            let Some(frame) = inbox.frame.as_ref() else { continue };
            let base = self.transfer + k * PER_INPUT;
            let made = (transfer::bits(cx.params, base), at);
            if !inbox.fresh && inbox.made == Some(made) {
                continue;
            }
            inbox.fresh = false;
            inbox.made = Some(made);
            if let Some(up) = transfer::read(cx.params, base).upload(frame, at, &mut inbox.range) {
                *self.uploads[k].lock().expect("the upload cell") = Some(up);
            }
        }
    }
}

impl Half for GraphicsHalf {
    /// An arrival replaces whatever the last tick has not drawn yet: latest wins, as every
    /// crossing into a scheduled engine is.
    fn arrive(&mut self, inbox: usize, frame: &Data) -> bool {
        if let Some(slot) = self.inboxes.get_mut(inbox) {
            slot.frame = Some(frame.clone());
            slot.fresh = true;
        }
        false
    }

    fn tick(&mut self, cx: &Cx<'_>, publish: &mut dyn FnMut(usize, &[u8])) -> Ticked {
        // What the render thread reads to decide whether this node runs at all.
        let readers = cx.readers.first().copied().unwrap_or(false);
        self.readers.store(readers, Ordering::Relaxed);
        // Taken from UNDER the lock and encoded outside it: the render thread waits on this
        // mutex, so an encode held across it is the frontend stalling a node tick.
        let taken = self.tap.lock().expect("the tap").frame.take();
        if let Some(frame) = taken {
            publish(0, &goofi_codec::encode(&frame));
        }
        self.tap.lock().expect("the tap").wanted = readers;
        let size = crate::plan::asked(cx.params, self.size);
        self.transfer(cx, transfer::drawn_size(size));
        Ticked { errors: Vec::new(), replan: std::mem::replace(&mut self.last, size) != size }
    }
}
