//! The graphics engine's executor on a node's runtime: an arrival becomes texels the render
//! thread uploads, and the frame that thread read back goes out while anyone drinks from it. A
//! host producer runs here too, on the shared host executor, and what it renders is the source.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use goofi_supervisor::sync::Mutex;

use goofi_runtime::{Cx, Executor, Fault, HostExecutor, Out, Ticked};
use goofi_node::NodeStage;
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

/// The cells one node's half, the render thread and the engine share, made once at insert.
pub struct Cells {
    pub params: Arc<[AtomicU64]>,
    /// Per ARRAY input: the frame the render thread has not uploaded yet.
    pub uploads: Vec<Mutex<Option<Upload>>>,
    /// The size of the last frame uploaded to the first ARRAY input, packed; what a zero axis
    /// of `common` follows when no texture is wired behind the node.
    pub uploaded: AtomicU64,
    /// Whether anyone drinks from this output right now: the half writes it each tick.
    pub readers: AtomicBool,
    /// The frame the render thread read back, until the half takes it. Latest wins: the ring's
    /// one slot in flight paces the readback.
    pub tap: Mutex<Option<Tapped>>,
    /// The box the readers want the readback fitted into, LIVE: a viewer never re-plans.
    pub tap_box: AtomicU64,
}

pub struct GraphicsHalf {
    producer: Option<(HostExecutor, crate::producer::Source)>,
    cells: Arc<Cells>,
    /// Where the universal `common` size starts in the param atomics.
    size: usize,
    /// The sizes last seen: asked, and uploaded. Only a settle can re-plan a stage's target, so
    /// the half — which ticks beside the writers of both — is what asks for one when either moves.
    last: ((u32, u32), u64),
    /// Why the last frame taken could not cross, until one does.
    refused: Option<String>,
}

/// The upload size as one atomic word, and back; 0 is no upload yet.
pub(crate) fn pack(size: (u32, u32)) -> u64 {
    (u64::from(size.0) << 32) | u64::from(size.1)
}

pub(crate) fn unpack(word: u64) -> Option<(u32, u32)> {
    (word != 0).then_some(((word >> 32) as u32, word as u32))
}

impl GraphicsHalf {
    pub fn new(cells: Arc<Cells>, size: usize, producer: Option<(HostExecutor, crate::producer::Source)>) -> GraphicsHalf {
        GraphicsHalf { producer, cells, size, last: ((u32::MAX, u32::MAX), 0), refused: None }
    }
}

impl Executor for GraphicsHalf {
    fn latest_only(&self) -> bool {
        true
    }

    /// An arrival replaces whatever the render thread has not taken yet: latest wins, as every
    /// crossing into a scheduled engine is.
    fn arrive(&mut self, inbox: usize, wire: usize, frame: &Data) -> bool {
        if let Some((producer, _)) = &mut self.producer { return producer.arrive(inbox, wire, frame); }
        let Some(cell) = self.cells.uploads.get(inbox) else { return false };
        if let Some(up) = Upload::of(frame) {
            if inbox == 0 {
                self.cells.uploaded.store(pack((up.width, up.height)), Ordering::Relaxed);
            }
            *cell.lock() = Some(up);
        }
        false
    }

    fn rewire(&mut self, inbox: usize, wires: &[(String, String)]) {
        if let Some((producer, _)) = &mut self.producer { producer.rewire(inbox, wires); }
        if inbox == 0 && wires.is_empty() {
            self.cells.uploaded.store(0, Ordering::Relaxed);
        }
    }
    fn params_changed(&mut self, values: &[goofi_core::Param]) -> Ticked {
        match &mut self.producer {
            Some((producer, _)) => producer.params_changed(values),
            None => Ticked::default(),
        }
    }
    fn pulse(&mut self, param: usize) -> Ticked {
        match &mut self.producer {
            Some((producer, _)) => producer.pulse(param),
            None => Ticked::default(),
        }
    }
    fn refresh(&mut self, param: usize) -> Option<Vec<String>> {
        self.producer.as_mut().and_then(|(producer, _)| producer.refresh(param))
    }
    /// The tap's pace, and the producer's own when it is due sooner.
    fn next_wake(&self, last_run: Instant) -> Option<Instant> {
        let tap = last_run + goofi_runtime::TICK;
        Some(self.producer.as_ref().and_then(|(p, _)| p.next_wake(last_run)).map_or(tap, |due| due.min(tap)))
    }
    fn fault(&self) -> Option<Fault> {
        self.refused.clone().map(Fault::Process).or_else(|| self.producer.as_ref().and_then(|(p, _)| p.fault()))
    }
    fn stage(&self) -> NodeStage {
        self.producer.as_ref().map_or(NodeStage::Ready, |(p, _)| p.stage())
    }
    fn run(&mut self, cx: &Cx<'_>, publish: &mut dyn FnMut(usize, Out<'_>)) -> Ticked {
        let mut ticked = Ticked::default();
        if let Some((producer, source)) = &mut self.producer {
            let now = Instant::now();
            if producer.next_wake(now).is_some_and(|due| due <= now) {
                let mut moved = false;
                ticked = producer.run(cx, &mut |_, out| {
                    if let Out::Frame(frame) = out {
                        moved |= crate::producer::produced(source, frame);
                    }
                });
                ticked.replan |= moved;
            }
        }
        // What the render thread reads to decide whether this node runs at all.
        let readers = cx.readers.first().copied().unwrap_or(false);
        self.cells.readers.store(readers, Ordering::Relaxed);
        // Taken from UNDER the lock and encoded outside it: the render thread waits on this
        // mutex, so an encode held across it is the frontend stalling a node tick.
        let taken = self.cells.tap.lock().take();
        let encoded = match taken {
            Some(Tapped::Full(frame)) => Some(goofi_codec::encode(&frame)),
            Some(Tapped::Texels { shape, bytes, meta }) => Some(goofi_codec::encode_u8(&shape, &bytes, &meta)),
            None => None,
        };
        match encoded {
            Some(Ok(bytes)) => {
                self.refused = None;
                publish(0, Out::Bytes(&bytes));
            }
            Some(Err(why)) => self.refused = Some(format!("the output frame cannot cross: {why}")),
            None => {}
        }
        let seen = (crate::plan::asked(cx.params, self.size), self.cells.uploaded.load(Ordering::Relaxed));
        ticked.replan |= std::mem::replace(&mut self.last, seen) != seen;
        ticked
    }
}
