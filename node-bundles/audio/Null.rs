//! Null — audio passed through whole. A fixed point to name: expressions reference it with
//! `nd()`, and the chain behind it can be rebuilt without touching the mappings.

use goofi_audio_sdk::goofi_core::SlotType;
use goofi_audio_sdk::{AudioNode, Block, Manifest, OutputDecl, SlotDecl, Tag};

static INS: &[SlotDecl] =
    &[SlotDecl { name: "input", kind: SlotType::Audio, trigger_process: false, multi: false, required: false }];
static OUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Audio }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Control],
    doc: "Audio passed through whole, every channel kept.\n\
          Name it and map params to it with `nd()`; the chain behind it can change freely.",
    inputs: INS,
    outputs: OUTS,
    params: &[],
};

#[derive(Default)]
struct Null;

impl AudioNode for Null {
    fn prepare(&mut self, _rate: f64) {}

    fn process(&mut self, b: &mut Block<'_>) {
        let input = &b.ins[0];
        let out = &mut b.outs[0];
        for c in 0..out.channels() as usize {
            out.chan_mut(c).copy_from_slice(input.chan(c));
        }
    }
}

goofi_audio_sdk::export!(Null, MANIFEST);
