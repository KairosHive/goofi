use goofi_sdk::audio::goofi_core::SlotType;
use goofi_sdk::audio::{Manifest, OutputDecl, Tag};

pub const TYPE: &str = "AudioIn";

goofi_sdk::audio::params! {
    DEVICE = crate::nodes::device("the input device; one other than the clock's drifts, and the ring holds or drops at its edges — an `ASIO: ` name must be the same driver the rest of the patch uses, since only one loads at a time"),
    CHANNELS = crate::chanmap::PARAM,
}

static OUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Audio }];

pub static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Input],
    doc: "The device's input: the channels `channels` names, in order, or all of them.",
    inputs: &[],
    outputs: OUTS,
    params: PARAMS,
};
