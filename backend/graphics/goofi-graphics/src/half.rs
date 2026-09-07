//! The graphics engine's half of a node's control thread: an arrival becomes texels the render
//! thread uploads, and the frame that thread read back goes out while anyone drinks from it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use goofi_control::{Cx, Half, Ticked};
use goofi_core::{Data, Value};

/// One arrival as the render thread takes it: `width * height * 4` f16 texels, row 0 the top.
pub struct Upload {
    pub width: u32,
    pub height: u32,
    pub texels: Vec<u16>,
}

impl Upload {
    /// A frame as RGBA texels, unclamped. `[N]` is one row; `[H, W]` is gray; `[H, W, C]` fills
    /// the channels it has, with alpha 1 where it has none.
    pub fn of(frame: &Data) -> Option<Upload> {
        let Value::Array(a) = frame.value() else { return None };
        let (h, w, c) = match *a.shape() {
            [n] => (1, n, 1),
            [h, w] => (h, w, 1),
            [h, w, c] if (1..=4).contains(&c) => (h, w, c),
            _ => return None,
        };
        // A texture the device cannot make invalidates the whole frame's command buffer, so a
        // frame past the limit is no upload at all.
        if h == 0 || w == 0 || h > crate::plan::MAX_SIZE as usize || w > crate::plan::MAX_SIZE as usize {
            return None;
        }
        let x: Vec<f32> =
            a.as_bytes().chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().expect("four bytes"))).collect();
        let mut texels = Vec::with_capacity(h * w * 4);
        for i in 0..h * w {
            let s = &x[i * c..(i + 1) * c];
            let rgba = match c {
                1 => [s[0], s[0], s[0], 1.0],
                2 => [s[0], s[0], s[0], s[1]],
                3 => [s[0], s[1], s[2], 1.0],
                _ => [s[0], s[1], s[2], s[3]],
            };
            texels.extend(rgba.iter().map(|v| half::f16::from_f32(if v.is_finite() { *v } else { 0.0 }).to_bits()));
        }
        Some(Upload { width: w as u32, height: h as u32, texels })
    }
}

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

pub struct GraphicsHalf {
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
    pub fn new(
        uploads: Vec<Arc<Mutex<Option<Upload>>>>,
        readers: Arc<AtomicBool>,
        tap: Arc<Mutex<Tap>>,
        size: usize,
    ) -> GraphicsHalf {
        GraphicsHalf { uploads, readers, tap, size, last: (u32::MAX, u32::MAX) }
    }
}

impl Half for GraphicsHalf {
    /// An arrival replaces whatever the render thread has not taken yet: latest wins, as every
    /// crossing into a scheduled engine is.
    fn arrive(&mut self, inbox: usize, frame: &Data) -> bool {
        let Some(cell) = self.uploads.get(inbox) else { return false };
        if let Some(up) = Upload::of(frame) {
            *cell.lock().unwrap() = Some(up);
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
        Ticked { errors: Vec::new(), replan: std::mem::replace(&mut self.last, size) != size }
    }
}
