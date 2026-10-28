//! What the manager tells the recorder: which outputs the settled graph holds armed, and what the
//! engines must do at a take's boundaries. The recorder owns every stream from there.

use std::sync::{Arc, Weak};

use goofi_graph::Graph;
use goofi_record::{Armed, Timeline};
use goofi_supervisor::sync::Mutex;

/// Every armed output the settled graph names, with the queue its CURRENT generation publishes
/// on: a signal node reads the clock at its `process`, a graphics stage at its tick, and the
/// audio engine counts samples — so one timeline carries the scheduler's jitter and the other
/// cannot.
pub(crate) fn armed(g: &Graph) -> Vec<Armed> {
    let mut out = Vec::new();
    for uid in g.all_uids() {
        for output in g.recorded(uid).unwrap_or(&[]) {
            let id = crate::ops::record::stream_id(g, uid, &output.slot);
            let base = goofi_transport::service_base(g.instance(), uid, g.node_generation(uid));
            out.push(Armed {
                service: goofi_transport::record_service(&base, &output.slot, output.serial),
                depth: goofi_transport::record_depth(id.engine),
                timeline: if id.engine == "audio" { Timeline::Derived } else { Timeline::Measured },
                quality: output.quality,
                id,
            });
        }
    }
    out
}

/// The engines behind the graph, as a take's start and stop reach them.
pub(crate) struct Engines(pub Weak<Mutex<Graph>>);

impl goofi_record::Engines for Engines {
    /// One acknowledged tick of every control half that holds frames of its own before the
    /// queue: the audio halves' block rings and the graphics halves' readbacks.
    fn flush(&self) -> Result<(), String> {
        let graph = self.0.upgrade().ok_or("the recording graph is gone")?;
        let waits = {
            let mut graph = graph.lock();
            let mut waits = crate::try_audio_engine(&mut graph).map(|audio| audio.flush_recording()).unwrap_or_default();
            waits.extend(crate::try_graphics_engine(&mut graph).map(|graphics| graphics.flush_recording()).unwrap_or_default());
            waits
        };
        for wait in waits {
            wait.recv().map_err(|_| "a node's runtime is gone")??;
        }
        Ok(())
    }

    fn audio_blocks(&self) -> Option<u64> {
        let graph = self.0.upgrade()?;
        let mut graph = graph.lock();
        crate::try_audio_engine(&mut graph).map(|audio| audio.blocks())
    }
}

/// Tell the recorder what the graph holds armed. Called after every settle, which is the one
/// place an arming, a disarm, a rebirth or a quality change becomes settled state.
pub(crate) fn follow(state: &crate::AppState, g: &Graph) {
    state.recorder.arm(armed(g));
}

/// The engines handed to the recorder once the graph is shared.
pub(crate) fn attach(recorder: &goofi_record::Recorder, graph: &Arc<Mutex<Graph>>) {
    recorder.set_engines(Arc::new(Engines(Arc::downgrade(graph))));
}
