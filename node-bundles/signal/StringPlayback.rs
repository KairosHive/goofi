//! StringPlayback — a recorded `.txt` stream played again, one line a frame at its recorded instant,
//! with the meta it was recorded with.

use goofi_core::replay::{open, Fallback, Kind, Player};
use goofi_core::SlotType;
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamSpec, Params, Tag};

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
        group: "playback",
        name: "file",
        spec: ParamSpec::Str { default: "", options: &[], refresh: false },
        expression: None,
        doc: Some("The recorded `.txt` file. Its `.jsonl` sidecar beside it gives each line's instant and meta."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "playback",
        name: "loop",
        spec: ParamSpec::Bool { default: true },
        expression: None,
        doc: Some("Start over at the end, instead of falling silent."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "fallback",
        name: "sfreq",
        spec: ParamSpec::Num { default: &[1.0], min: 0.001, max: 100_000.0, int: false, options: &[], color: false },
        expression: None,
        doc: Some("Without a sidecar: the lines per second."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "common",
        name: "max_frequency",
        spec: ParamSpec::Num { default: &[500.0], min: 0.0, max: 1000.0, int: false, options: &[], color: false },
        expression: None,
        doc: Some("How often to look for a due frame. One frame leaves per run, so this bounds the replayed rate."),
        section: 0,
        show: &[],
        role: None,
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

goofi_signal_sdk::export!(StringPlayback, MANIFEST);
