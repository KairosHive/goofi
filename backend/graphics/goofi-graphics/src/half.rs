//! The graphics engine's executor on a node's runtime: an arrival becomes texels the render
//! thread uploads, and the frame that thread read back goes out while anyone drinks from it.
//!
//! Every crossing between this thread and the render thread is a cell with no lock on either
//! side: the render thread hands a readback over as the mapped GPU buffer it is, and this thread
//! copies it out, encodes it and publishes it. What a reader costs is paid here, never on the
//! clock that draws. The one crossing that waits is a [`Meeting`]: a node the tick calls runs
//! here, and the tick stands at its place until it has.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use goofi_runtime::{Cx, Executor, Fault, HostExecutor, Out, Ticked};
use goofi_core::texture::{PixelFormat, Pixels, Texture};
use goofi_core::{Data, Meta};
use goofi_supervisor::sync::{Condvar, Mailbox, Mutex, Stack};
pub use crate::resources::Upload;
use crate::gpu::Want;
use crate::resources::Readback;

/// The cells one node's half, the render thread and the engine share, made once at insert.
pub struct Cells {
    pub params: Arc<[AtomicU64]>,
    /// Per input a wire can feed (the ARRAY inputs, then the TEXTURE inputs): the frame the render
    /// thread has not uploaded yet. Latest wins, and the half frees what a new frame displaced.
    pub uploads: Vec<Mailbox<Upload>>,
    /// The uploads the render thread is done with, so their buffers are freed and reused here.
    pub spent: Stack<Box<Upload>>,
    /// The size of the last frame uploaded to each of `uploads`, packed, and 0 while no wire
    /// feeds it; what a zero axis of `common` follows when no stage is behind the node.
    pub uploaded: Vec<AtomicU64>,
    /// Whether anyone drinks from this output right now: the half writes it each tick.
    pub readers: AtomicBool,
    /// The frame the render thread read back for the viewers, until the half takes it. Latest
    /// wins: a frame the half did not take in time gives its slot back.
    pub tap: Mailbox<Readback>,
    /// The box the readers want the readback fitted into, LIVE: a viewer never re-plans.
    pub tap_box: AtomicU64,
    /// The frames the render thread read back for the recording, until the half hands them to
    /// its queue. Every one: the ring's slots bound how many are ever out at once.
    pub record: Stack<Readback>,
    /// The fraction of the frame period this node's pass took, as `f64` bits the render thread
    /// writes: not a number until a pass is timed, and zero while no one asks for the stage.
    pub frame_share: AtomicU64,
    /// For a node the tick calls: where the render thread asks for a run and waits for it.
    pub node: Option<Meeting>,
    /// The size of the picture such a node last made, packed; 0 until it makes one.
    pub produced: AtomicU64,
    /// This half's door, rung by the render thread once a readback or a call is in a cell above.
    pub bell: goofi_transport::Doorbell,
    /// The render clock's door and the recorder's flag: the half rings when a reader or a
    /// recording arrives, because a clock with nothing to draw is parked.
    pub wake: Arc<goofi_supervisor::timing::Event>,
    pub recording: Arc<AtomicBool>,
}

/// One run's inputs: per input another stage feeds, its frame, or none where it is unwired.
pub type Asked = Vec<(usize, Option<Data>)>;

/// Where the render thread calls a node's half and waits for the run.
pub struct Meeting {
    /// Whether the node is built and takes calls; until then the tick passes its stage by.
    pub ready: AtomicBool,
    /// Where the node's child joins, on a device that shares memory with it.
    pub door: Option<crate::share::Door>,
    /// Whether the child joined: its pictures then stay in device memory.
    pub linked: AtomicBool,
    asked: Mailbox<Asked>,
    answer: Mutex<Answer>,
    answered: Condvar,
}

#[derive(Default)]
struct Answer {
    /// What the run made; `Some(None)` where it made no picture.
    made: Option<Option<Data>>,
    /// The half is gone or the engine stops: no answer comes, and a waiting tick goes on.
    closed: bool,
}

impl Meeting {
    pub fn new(door: Option<crate::share::Door>) -> Meeting {
        Meeting {
            ready: AtomicBool::new(false),
            door,
            linked: AtomicBool::new(false),
            asked: Mailbox::new(),
            answer: Mutex::new(Answer::default()),
            answered: Condvar::new(),
        }
    }

    /// Run the node on `inputs` and wait for its picture. None where it made none, where the
    /// run failed (the node says why), or where the meeting is closed.
    pub fn call(&self, bell: &goofi_transport::Doorbell, inputs: Asked) -> Option<Data> {
        self.answer.lock().made = None;
        drop(self.asked.put(Box::new(inputs)));
        bell.ring(0);
        let mut answer = self.answered.wait_while(self.answer.lock(), |answer| answer.made.is_none() && !answer.closed);
        answer.made.take().flatten()
    }

    fn answer(&self, made: Option<Data>) {
        self.answer.lock().made = Some(made);
        self.answered.notify_all();
    }

    /// End the meeting: a tick that waits goes on with nothing, and so does every later call.
    pub fn close(&self) {
        self.answer.lock().closed = true;
        self.answered.notify_all();
    }
}

pub struct GraphicsHalf {
    cells: Arc<Cells>,
    /// Where the universal `common` size starts in the param atomics.
    size: usize,
    /// The sizes last seen: asked, and uploaded. Only a settle can re-plan a stage's target, so
    /// the half — which ticks beside the writers of both — is what asks for one when either moves.
    last: ((u32, u32), Vec<u64>),
    /// Why the last frame taken could not cross, until one does.
    refused: Option<String>,
    /// Texel buffers the render thread gave back, for the next arrival to fill.
    spare: Vec<Vec<u8>>,
    /// The node the tick calls, built on this thread, and the declared input behind each inbox.
    node: Option<(HostExecutor, Vec<usize>)>,
    /// Whether a reader or a recording asked for this stage at the last run.
    asked: bool,
}

/// The upload size as one atomic word, and back; 0 is no upload yet.
pub(crate) fn pack(size: (u32, u32)) -> u64 {
    (u64::from(size.0) << 32) | u64::from(size.1)
}

pub(crate) fn unpack(word: u64) -> Option<(u32, u32)> {
    (word != 0).then_some(((word >> 32) as u32, word as u32))
}

impl GraphicsHalf {
    pub fn new(cells: Arc<Cells>, size: usize, node: Option<(HostExecutor, Vec<usize>)>) -> GraphicsHalf {
        if let Some(meeting) = &cells.node {
            meeting.ready.store(true, Ordering::Release);
        }
        GraphicsHalf { cells, size, last: ((u32::MAX, u32::MAX), Vec::new()), refused: None, spare: Vec::new(), node, asked: false }
    }

    /// Whether the render thread left something for this thread to take.
    fn owed(&self) -> bool {
        !self.cells.tap.is_empty() || !self.cells.record.is_empty() || self.cells.node.as_ref().is_some_and(|meeting| !meeting.asked.is_empty())
    }

    /// The run the render thread asked for and waits on: its inputs in, the picture back.
    fn meet(&mut self, cx: &Cx<'_>, ticked: &mut Ticked) {
        let (Some((node, _)), Some(meeting)) = (self.node.as_mut(), self.cells.node.as_ref()) else { return };
        let Some(inputs) = meeting.asked.take() else { return };
        for (slot, frame) in *inputs {
            match frame {
                Some(frame) => {
                    node.arrive(slot, 0, &frame);
                }
                None => node.rewire(slot, &[]),
            }
        }
        let mut made = None;
        let ran = node.run(cx, &mut |_, out| {
            if let Out::Frame(frame) = out {
                made = Some(frame.clone());
            }
        });
        ticked.errors = ran.errors;
        meeting.answer(made);
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
            meta.set_procstart(Some(readback.at));
            let texels = Arc::new(goofi_core::record::Texels { width, height, rgba, meta });
            publish(0, Out::Record(goofi_core::record::Frame::Texels(texels)));
        }
    }

    /// The readers' readback: a full frame goes as the texture it is; the 8-bit texels go as the
    /// GOOF frame a viewer forwards. Either way the rows are copied off the mapped buffer once.
    fn tap(&mut self, publish: &mut dyn FnMut(usize, Out<'_>)) {
        let Some(readback) = self.cells.tap.take() else { return };
        let (w, h) = readback.size();
        let shape = vec![h as usize, w as usize, 4];
        // The frame is stamped with the tick that drew it, so a reader tells a new frame from the
        // one it already holds without comparing texels. A frame that was shrunk on the way out
        // says so, in the words `reduce_for_view` uses — otherwise it would understate its own
        // origin and a viewer could not map a texel back to the pixel it came from.
        let mut meta = Meta::new();
        meta.set_procstart(Some(readback.at));
        meta.set_procend(Some(readback.at));
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
                match goofi_codec::encode_u8_with(&shape, &meta, |frame| {
                    readback.rows_into(frame);
                }) {
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
                let pixels = Pixels { width: w, height: h, stride: w as usize * 16, format: PixelFormat::Rgba32Float, bytes: rows };
                if let Ok(frame) = Data::texture(Texture::Pixels(pixels), meta) {
                    self.refused = None;
                    publish(0, Out::Wire(&frame));
                }
            }
        }
    }
}

impl Drop for GraphicsHalf {
    fn drop(&mut self) {
        if let Some(meeting) = &self.cells.node {
            meeting.close();
        }
    }
}

impl Executor for GraphicsHalf {
    fn latest_only(&self) -> bool {
        true
    }

    /// An arrival replaces whatever the render thread has not taken yet: latest wins, as every
    /// crossing into a scheduled engine is. A node the tick calls holds the frame for its next run.
    fn arrive(&mut self, inbox: usize, wire: usize, frame: &Data) -> bool {
        if let Some((node, slots)) = self.node.as_mut() {
            if let Some(slot) = slots.get(inbox) {
                node.arrive(*slot, wire, frame);
            }
            return false;
        }
        let Some(cell) = self.cells.uploads.get(inbox) else { return false };
        // What the render thread is done with comes back here, where its buffer is freed or
        // filled again: an upload costs the clock a pointer move each way.
        self.spare.extend(self.cells.spent.drain().map(|up| up.texels));
        let made = match frame.value() {
            goofi_core::Value::Texture(texture) => texture.pixels().ok().map(Upload::pixels),
            _ => Upload::of(frame, self.spare.pop().unwrap_or_default()),
        };
        if let Some(up) = made {
            if let Some(size) = self.cells.uploaded.get(inbox) {
                size.store(pack((up.width, up.height)), Ordering::Relaxed);
            }
            if let Some(displaced) = cell.put(Box::new(up)) {
                self.spare.push(displaced.texels);
            }
            self.spare.truncate(2);
        }
        false
    }

    fn rewire(&mut self, inbox: usize, wires: &[(String, String)]) {
        if let Some((node, slots)) = self.node.as_mut() {
            if let Some(slot) = slots.get(inbox) {
                node.rewire(*slot, wires);
            }
            return;
        }
        if let (true, Some(size)) = (wires.is_empty(), self.cells.uploaded.get(inbox)) {
            size.store(0, Ordering::Relaxed);
        }
    }
    fn params_changed(&mut self, values: &[goofi_core::Param]) -> Ticked {
        self.node.as_mut().map_or_else(Ticked::default, |(node, _)| node.params_changed(values))
    }
    fn pulse(&mut self, param: usize) -> Ticked {
        self.node.as_mut().map_or_else(Ticked::default, |(node, _)| node.pulse(param))
    }
    fn refresh(&mut self, param: usize) -> Option<Vec<String>> {
        self.node.as_mut().and_then(|(node, _)| node.refresh(param))
    }
    fn stage(&self) -> goofi_node::NodeStage {
        self.node.as_ref().map_or(goofi_node::NodeStage::Ready, |(node, _)| node.stage())
    }
    /// Now, while the render thread has left a readback to take; else the tap's pace.
    fn next_wake(&self, last_run: Instant) -> Option<Instant> {
        Some(if self.owed() { last_run } else { last_run + goofi_runtime::TICK })
    }
    fn fault(&self) -> Option<Fault> {
        self.node.as_ref().and_then(|(node, _)| node.fault()).or_else(|| self.refused.clone().map(Fault::Process))
    }
    fn run(&mut self, cx: &Cx<'_>, publish: &mut dyn FnMut(usize, Out<'_>)) -> Ticked {
        let mut ticked = Ticked::default();
        // What the render thread reads to decide whether this node runs at all.
        let readers = cx.readers.first().copied().unwrap_or(false);
        self.cells.readers.store(readers, Ordering::Relaxed);
        let asked = readers || self.cells.recording.load(Ordering::Relaxed);
        if asked && !std::mem::replace(&mut self.asked, asked) {
            self.cells.wake.ring(0);
        }
        self.asked = asked;
        self.meet(cx, &mut ticked);
        self.record(publish);
        self.tap(publish);
        let asked = crate::plan::asked(cx.params, self.size);
        let uploaded = self.cells.uploaded.iter().chain([&self.cells.produced]).map(|size| size.load(Ordering::Relaxed));
        if asked != self.last.0 || !uploaded.clone().eq(self.last.1.iter().copied()) {
            self.last = (asked, uploaded.collect());
            ticked.replan = true;
        }
        let share = f64::from_bits(self.cells.frame_share.load(Ordering::Relaxed));
        ticked.frame_share = share.is_finite().then_some(share);
        ticked
    }
}
