//! StringPlayback — a recorded `.txt` stream played again, one line a frame at its recorded instant,
//! with the meta it was recorded with.

use goofi_core::replay::{open, Fallback, Kind, Player};
use goofi_core::SlotType;
use goofi_sdk::signal::{Face, Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamSpec, Params, Tag};

/// The file and fallback a player was opened with, and what opening it gave.
type Loaded = ((String, usize, u64), Result<Player, String>);

#[derive(Default)]
struct StringPlayback {
    loaded: Option<Loaded>,
}

impl Node for StringPlayback {
    fn process(&mut self, _inp: &Inputs<'_>, out: &mut Outputs<'_>, c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let file = p.str("playback", "file").unwrap_or("").trim().to_string();
        if file.is_empty() {
            return Ok(());
        }
        let fallback = Fallback {
            channels: 1,
            sfreq: p.f64("fallback", "sfreq").unwrap_or(1.0).max(1e-3),
        };
        let key = (file, fallback.channels, fallback.sfreq.to_bits());
        if self.loaded.as_ref().is_none_or(|(k, _)| *k != key) {
            let player = open(std::path::Path::new(&key.0), fallback).and_then(|r| match r.kind {
                Kind::Text => Ok(Player::new(r)),
                _ => Err("not a text recording: use ArrayPlayback or TablePlayback".into()),
            });
            self.loaded = Some((key, player));
        }
        let player = self.loaded.as_mut().unwrap().1.as_mut().map_err(|e| e.clone())?;
        if let Some(frame) = player.due(c.now, p.bool("playback", "loop").unwrap_or(false)) {
            out.set("out", frame);
        }
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        doc: Some("The recorded `.txt` file. Its `.jsonl` sidecar beside it gives each line's instant and meta."),
        ..ParamDecl::new("playback", "file", ParamSpec::Str { default: "", options: &[], refresh: false })
    },
    ParamDecl {
        doc: Some("Start over at the end, instead of falling silent."),
        ..ParamDecl::new("playback", "loop", ParamSpec::Bool { default: true })
    },
    ParamDecl {
        doc: Some("Without a sidecar: the lines per second."),
        ..ParamDecl::new("fallback", "sfreq", ParamSpec::Num { default: &[1.0], min: 0.001, max: 100_000.0, int: false, options: &[], face: Face::Plain })
    },
    ParamDecl {
        doc: Some("How often to look for a due frame. One frame leaves per run, so this bounds the replayed rate."),
        ..ParamDecl::new("common", "max_frequency", ParamSpec::Num { default: &[500.0], min: 0.0, max: 1000.0, int: false, options: &[], face: Face::Plain })
    },
];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::String }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator],
    doc: "Plays a recorded text stream again, line by line at its recorded instants.",
    inputs: &[],
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_sdk::signal::export!(StringPlayback, MANIFEST);
