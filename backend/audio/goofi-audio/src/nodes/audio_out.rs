use goofi_audio_sdk::goofi_core::SlotType;
use goofi_audio_sdk::{AudioNode, Block, Manifest, ParamDecl, ParamSpec, SlotDecl, Tag};

use crate::nodes::Birth;

pub const TYPE: &str = "AudioOut";

goofi_audio_sdk::params! {
    DEVICE = ParamDecl {
        group: "audio",
        name: "device",
        spec: ParamSpec::Str { default: crate::DEFAULT_DEVICE, options: &[crate::DEFAULT_DEVICE], refresh: true },
        expression: None,
        doc: Some("the output device the engine's clock follows; every AudioOut names the same one"),
    },
    CHANNELS = ParamDecl {
        group: "audio",
        name: "channels",
        spec: ParamSpec::Str { default: crate::chanmap::ALL, options: &[], refresh: false },
        expression: None,
        doc: Some(crate::chanmap::DOC),
    },
    GAIN = ParamDecl {
        group: "audio",
        name: "gain",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 10.0 },
        expression: None,
        doc: None,
    },
}

static INS: &[SlotDecl] =
    &[SlotDecl { name: "input", kind: SlotType::Audio, trigger_process: false, multi: true, required: false }];

pub static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Output],
    doc: "The sound device: what reaches `input` is heard, times `gain`.\n\
          `channels` names the device channels it lands on, the first incoming channel on the \
          first channel named, so two AudioOuts can hold different pairs of one card.\n\
          Every AudioOut on the device sums.",
    inputs: INS,
    outputs: &[],
    params: PARAMS,
};

/// The device is the clock and the sum; the node itself holds nothing.
pub struct AudioOut;

impl AudioOut {
    pub fn new(_birth: Birth) -> AudioOut {
        AudioOut
    }
}

impl AudioNode for AudioOut {
    fn audio_params(&self, _declared: usize) -> usize {
        0
    }

    fn prepare(&mut self, _rate: f64) {}

    fn process(&mut self, _b: &mut Block<'_>) {}
}
