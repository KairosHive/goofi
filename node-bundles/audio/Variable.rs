//! Variable — a variable's frame crossing into the audio plane. The named variable's wire feeds
//! `value` as a cable would; the crossing plays each frame as a waveform, a `[1]` value as a level.

use goofi_audio_sdk::goofi_core::SlotType;
use goofi_audio_sdk::{AudioNode, Block, Manifest, OutputDecl, ParamDecl, ParamSpec, Role, SlotDecl, Tag};

goofi_audio_sdk::params! {
    NAME = ParamDecl {
        group: "variable",
        name: "name",
        spec: ParamSpec::Str { default: "", options: &[], refresh: true },
        expression: None,
        doc: Some("the variable to read, `group.element`"),
        section: 0,
        show: None,
        role: Some(Role::Feed { slot: "value" }),
    },
}

static INS: &[SlotDecl] =
    &[SlotDecl { name: "value", kind: SlotType::Array, trigger_process: false, multi: false, required: false }];
static OUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Audio }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Input],
    doc: "A variable's frame on the audio plane.\n\
          `name` picks the variable; its frames enter as `SignalIn` plays a waveform, the value as \
          it stands: a `[1]` value is a level, a `[C, T]` frame is C channels of T samples looped \
          until the next.",
    inputs: INS,
    outputs: OUTS,
    params: PARAMS,
};

#[derive(Default)]
struct Variable;

impl AudioNode for Variable {
    fn prepare(&mut self, _rate: f64) {}

    /// The engine plays the frames; what reaches here is already sound.
    fn process(&mut self, b: &mut Block<'_>) {
        let input = &b.ins[0];
        let out = &mut b.outs[0];
        for c in 0..out.channels() as usize {
            out.chan_mut(c).copy_from_slice(input.chan(c));
        }
    }
}

goofi_audio_sdk::export!(Variable, MANIFEST);
