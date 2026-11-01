//! The graphics engine's executor on a node's runtime: an arrival becomes texels the render
//! thread uploads, and the frame that thread read back goes out while anyone drinks from it. A
//! host producer runs here too, on the shared host executor, and what it renders is the source.
//!
//! Every crossing between this thread and the render thread is a cell with no lock on either
//! side: the render thread hands a readback over as the mapped GPU buffer it is, and this thread
//! copies it out, encodes it and publishes it. What a reader costs is paid here, never on the
//! clock that draws.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use goofi_runtime::{Cx, Executor, Fault, HostExecutor, Out, Ticked};
use goofi_node::NodeStage;
use goofi_core::{Data, Meta};
use goofi_supervisor::sync::{Mailbox, Stack};
pub use crate::resources::Upload;
use crate::gpu::Want;
use crate::resources::Readback;

/// The cells one node's half, the render thread and the engine share, made once at insert.
pub struct Cells {
    pub params: Arc<[AtomicU64]>,
    /// Per ARRAY input: the frame the render thread has not uploaded yet. Latest wins: a frame
    /// that arrives before the last was taken displaces it, and the half frees what it displaced.
    pub uploads: Vec<Mailbox<Upload>>,
    /// The uploads the render thread is done with, so their buffers are freed and reused here.
    pub spent: Stack<Box<Upload>>,
    /// The size of the last frame uploaded to the first ARRAY input, packed; what a zero axis
    /// of `common` follows when no texture is wired behind the node.
    pub uploaded: AtomicU64,
    /// Whether anyone drinks from this output right now: the half writes it each tick.
    pub readers: AtomicBool,
    /// The frame the render thread read back for the viewers, until the half takes it. Latest
    /// wins: the ring's one slot in flight paces the readback.
    pub tap: Mailbox<Readback>,
    /// The box the readers want the readback fitted into, LIVE: a viewer never re-plans.
    pub tap_box: AtomicU64,
    /// The frames the render thread read back for the recording, until the half hands them to
    /// its queue. Every one: the ring's slots bound how many are ever out at once.
    pub record: Stack<Readback>,
    /// This half's door, rung by the render thread once a readback is in a cell above.
    pub bell: goofi_transport::Doorbell,
}

pub struct GraphicsHalf {
    producer: Option<(HostExecutor, crate::producer::Source)>,
    /// What the producer last put out, so a frame that moved nothing asks for no settle.
    produced: Option<crate::producer::Produced>,
    cells: Arc<Cells>,
    /// Where the universal `common` size starts in the param atomics.
    size: usize,
    /// The sizes last seen: asked, and uploaded. Only a settle can re-plan a stage's target, so
    /// the half — which ticks beside the writers of both — is what asks for one when either moves.
    last: ((u32, u32), u64),
    /// Why the last frame taken could not cross, until one does.
    refused: Option<String>,
    /// The 8-bit tap's rows, kept between two frames: the encoder copies them once more.
    scratch: Vec<u8>,
    /// Texel buffers the render thread gave back, for the next arrival to fill.
    spare: Vec<Vec<u8>>,
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
        GraphicsHalf { producer, produced: None, cells, size, last: ((u32::MAX, u32::MAX), 0), refused: None, scratch: Vec::new(), spare: Vec::new() }
    }

    /// Whether the render thread left something for this thread to take.
    fn owed(&self) -> bool {
        !self.cells.tap.is_empty() || !self.cells.record.is_empty()
    }

    /// Every readback for the recording, oldest first, as the frames the recorder files.
    fn record(&mut self, publish: &mut dyn FnMut(usize, Out<'_>)) {
        let mut taken: Vec<Readback> = self.cells.record.drain().collect();
        taken.sort_by_key(|r| r.index);
        for readback in taken {
            let mut rgba = Vec::new();
            if !readback.rows_into(&mut rgba) {
                continue;
            }
            let (width, height) = readback.size();
            let mut meta = Meta::new().with_sfreq(Some(readback.sfreq)).with_index(Some(readback.index));
            meta.set_time(Some(readback.at));
            let texels = Arc::new(goofi_core::record::Texels { width, height, rgba, meta });
            publish(0, Out::Record(goofi_core::record::Frame::Texels(texels)));
        }
    }

    /// The viewers' readback, as the frame their width asks for: a full frame goes as it is; the
    /// 8-bit texels go as the GOOF frame they are, which their readers forward.
    fn tap(&mut self, publish: &mut dyn FnMut(usize, Out<'_>)) {
        let Some(readback) = self.cells.tap.take() else { return };
        let (w, h) = readback.size();
        let shape = vec![h as usize, w as usize, 4];
        // A frame that was shrunk on the way out says so, in the words `reduce_for_view` uses —
        // otherwise it would understate its own origin and a viewer could not map a texel back
        // to the pixel it came from.
        let mut meta = Meta::new();
        let shrunk = readback.shrunk;
        if shrunk.from != shrunk.to {
            let axes = [(0, shrunk.from.1 as usize), (1, shrunk.from.0 as usize)];
            goofi_core::reduce::note_reduced(&mut meta, &axes);
        }
        match readback.want {
            Want::TapU8 => {
                // The window the GPU clamped to, said the way `quantize_u8` says it, so a viewer
                // maps a texel back through the one rule.
                goofi_core::reduce::note_depth(&mut meta, 0.0, 1.0);
                if !readback.rows_into(&mut self.scratch) {
                    return;
                }
                match goofi_codec::encode_u8(&shape, &self.scratch, &meta) {
                    Ok(encoded) => {
                        self.refused = None;
                        publish(0, Out::Bytes(bytes::Bytes::from(encoded)));
                    }
                    Err(why) => self.refused = Some(format!("the output frame cannot cross: {why}")),
                }
            }
            _ => {
                let mut rows = Vec::new();
                if !readback.rows_into(&mut rows) {
                    return;
                }
                if let Ok(frame) = Data::array_f32(shape, rows, meta) {
                    self.refused = None;
                    publish(0, Out::Wire(&frame));
                }
            }
        }
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
        // What the render thread is done with comes back here, where its buffer is freed or
        // filled again: an upload costs the clock a pointer move each way.
        self.spare.extend(self.cells.spent.drain().map(|up| up.texels));
        if let Some(up) = Upload::of(frame, self.spare.pop().unwrap_or_default()) {
            if inbox == 0 {
                self.cells.uploaded.store(pack((up.width, up.height)), Ordering::Relaxed);
            }
            if let Some(displaced) = cell.put(Box::new(up)) {
                self.spare.push(displaced.texels);
            }
            self.spare.truncate(2);
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
    /// Now, while the render thread has left a readback to take; else the tap's pace, and the
    /// producer's own when it is due sooner.
    fn next_wake(&self, last_run: Instant) -> Option<Instant> {
        if self.owed() {
            return Some(last_run);
        }
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
                let last = &mut self.produced;
                ticked = producer.run(cx, &mut |_, out| {
                    if let Out::Frame(frame) = out {
                        moved |= crate::producer::produced(source, last, frame);
                    }
                });
                ticked.replan |= moved;
            }
        }
        // What the render thread reads to decide whether this node runs at all.
        let readers = cx.readers.first().copied().unwrap_or(false);
        self.cells.readers.store(readers, Ordering::Relaxed);
        self.record(publish);
        self.tap(publish);
        let seen = (crate::plan::asked(cx.params, self.size), self.cells.uploaded.load(Ordering::Relaxed));
        ticked.replan |= std::mem::replace(&mut self.last, seen) != seen;
        ticked
    }
}
