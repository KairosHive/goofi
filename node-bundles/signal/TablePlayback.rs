//! TablePlayback — a recorded `.csv` stream played again, one row a frame at its `t`, with the meta
//! its sidecar holds. A cell that reads as a number is a number; any other is its text.

use goofi_core::replay::{open, Fallback, Kind, Player};
use goofi_core::SlotType;
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamSpec, Params, Tag};

/// The file and fallback a player was opened with, and what opening it gave.
type Loaded = ((String, usize, u64), Result<Player, String>);

#[derive(Default)]
struct TablePlayback {
    loaded: Option<Loaded>,
}

impl Node for TablePlayback {
    fn process(&mut self, _inp: &Inputs<'_>, out: &mut Outputs<'_>, c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let file = p.str("playback", "file").unwrap_or("").trim().to_string();
        if file.is_empty() {
            return Ok(());
        }
        // A CSV row carries its own instant, so nothing stands in for a sidecar.
        let fallback = Fallback { channels: 1, sfreq: 1.0 };
        let key = (file, fallback.channels, fallback.sfreq.to_bits());
        if self.loaded.as_ref().is_none_or(|(k, _)| *k != key) {
            let player = open(std::path::Path::new(&key.0), fallback).and_then(|r| match r.kind {
                Kind::Table => Ok(Player::new(r)),
                _ => Err("not a table recording: use ArrayPlayback or StringPlayback".into()),
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
        doc: Some("The recorded `.csv` file. A `.jsonl` sidecar beside it adds each row's meta."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "playback",
        name: "loop",
        spec: ParamSpec::Bool { default: false },
        expression: None,
        doc: Some("Start over at the end."),
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
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Table }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator],
    doc: "Plays a recorded table stream again, row by row at its recorded instants.",
    inputs: &[],
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(TablePlayback, MANIFEST);
