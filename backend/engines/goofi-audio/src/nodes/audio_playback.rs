use goofi_sdk::audio::goofi_core::SlotType;
use goofi_sdk::audio::{Face, Manifest, OutputDecl, ParamDecl, ParamSpec, Tag};

pub const TYPE: &str = "AudioPlayback";

goofi_sdk::audio::params! {
    FILE = ParamDecl {
        doc: Some("the WAV file to play; a bare name is looked for in the recordings folder"),
        ..ParamDecl::new("play", "file", ParamSpec::Str { default: "", options: &[], refresh: false })
    },
    POSITION = ParamDecl {
        doc: Some("where in the file to play from; playback runs on its own, and a MOVE of this skips"),
        ..ParamDecl::new("play", "position", ParamSpec::Num { default: &[0.0], min: 0.0, max: 1.0, int: false, options: &[], face: Face::Plain })
    },
    RESET = ParamDecl { doc: Some("play from the start again"), ..ParamDecl::new("play", "reset", ParamSpec::Pulse) },
    LOOPING = ParamDecl { doc: Some("start again at the end, instead of falling silent"), ..ParamDecl::new("play", "loop", ParamSpec::Bool { default: true }) },
}

static OUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Audio }];

pub static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Input],
    doc: "A WAV file as audio.\n\
          As many channels as the file holds, resampled to the engine's rate.",
    inputs: &[],
    outputs: OUTS,
    params: PARAMS,
};
