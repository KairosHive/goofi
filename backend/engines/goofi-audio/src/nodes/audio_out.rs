use goofi_sdk::audio::goofi_core::SlotType;
use goofi_sdk::audio::{Face, AudioNode, Block, Manifest, ParamDecl, ParamSpec, SlotDecl, Tag};

pub const TYPE: &str = "AudioOut";

goofi_sdk::audio::params! {
    DEVICE = crate::nodes::device("the output device the engine's clock follows; every AudioOut names the same one"),
    CHANNELS = crate::chanmap::PARAM,
    GAIN = ParamDecl::new("audio", "gain", ParamSpec::Num { default: &[1.0], min: 0.0, max: 10.0, int: false, options: &[], face: Face::Plain }),
}

static INS: &[SlotDecl] =
    &[SlotDecl { name: "input", kind: SlotType::Audio, trigger_process: false, multi: true, required: false }];

pub static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Output],
    doc: "The sound device: what reaches `input` is heard, times `gain`.\n\
          `channels` names the device channels it lands on, the first incoming channel on the \
          first channel named, so two AudioOuts can hold different pairs of one card.\n\
          Every AudioOut on the device sums.\n\
          The `browser` device is no sound card: each browser on the app plays it, at 48 kHz in \
          stereo, unless its sound button is off.\n\
          `default` is the machine's default sound card, and the `browser` device where the \
          machine has none: one patch is heard on a desk and on a server.",
    inputs: INS,
    outputs: &[],
    params: PARAMS,
};

/// The device is the clock and the sum; the node itself holds nothing.
pub struct AudioOut;

impl AudioNode for AudioOut {
    fn audio_params(&self, _declared: usize) -> usize {
        0
    }

    fn process(&mut self, _b: &mut Block<'_>) {}
}
