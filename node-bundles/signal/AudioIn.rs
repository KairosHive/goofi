//! AudioIn — the audio plane as frames. A tap hands over whatever the last blocks held, so a
//! frame's length is the CLOCK's rather than the analysis's; the modes here are the framing a
//! signal node wants, cut from the samples themselves.

use goofi_core::{Data, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, Params, ParamSpec, SlotDecl, Tag};

#[derive(Default)]
struct AudioIn {
    /// The samples no frame has carried yet, one lane per channel.
    held: Vec<Vec<f32>>,
}

fn read(bytes: &[u8], i: usize) -> f32 {
    f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("four bytes"))
}

impl Node for AudioIn {
    fn process(
        &mut self,
        inp: &Inputs<'_>,
        out: &mut Outputs<'_>,
        _c: &mut NodeCtx,
        p: &Params<'_>,
    ) -> NodeResult {
        let d = inp.get("input").ok_or("`input` is required")?;
        let a = d.assert_ndims().at_least(1)?;
        let shape = a.shape();
        let t = *shape.last().expect("at least one axis");
        let lanes: usize = shape[..shape.len() - 1].iter().product::<usize>().max(1);
        let mode = p.str("audio", "mode").unwrap_or("stream");
        if mode == "stream" {
            self.held.clear();
            out.set("out", d.clone());
            return Ok(());
        }
        let size = p.i64("audio", "size").unwrap_or(1024).clamp(1, 1_000_000) as usize;
        if self.held.len() != lanes {
            self.held = vec![Vec::new(); lanes];
        }
        let src = a.as_bytes();
        for (l, lane) in self.held.iter_mut().enumerate() {
            lane.extend((0..t).map(|i| read(src, l * t + i)));
        }
        let kept = self.held[0].len();
        let mut shape = shape[..shape.len() - 1].to_vec();
        let mut meta = d.meta().clone();
        let mut buf = Vec::new();
        if mode == "envelope" {
            let blocks = kept / size;
            if blocks == 0 {
                return Ok(());
            }
            for lane in &mut self.held {
                for b in 0..blocks {
                    let block = &lane[b * size..(b + 1) * size];
                    buf.extend((block.iter().map(|v| v * v).sum::<f32>() / size as f32).sqrt().to_le_bytes());
                }
                lane.drain(..blocks * size);
            }
            meta.set_sfreq(d.meta().sfreq().map(|sf| sf / size as f64));
            shape.push(blocks);
        } else {
            if kept < size {
                return Ok(());
            }
            // The newest window, whole. A window's worth is what a frame carries, so a stream
            // running faster than the patch reads it leaves the samples between two windows
            // behind rather than the newest ones — latest wins, as every crossing here is.
            for lane in &mut self.held {
                buf.extend(lane[lane.len() - size..].iter().flat_map(|v| v.to_le_bytes()));
                lane.clear();
            }
            shape.push(size);
        }
        out.set("out", Data::array_f32(shape, buf, meta).map_err(|e| e.to_string())?);
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "audio",
        name: "mode",
        spec: ParamSpec::Str { default: "stream", options: &["stream", "window", "envelope"], refresh: false },
        expression: None,
        doc: Some(
            "The samples as the tap hands them over, cut into windows of `size`, or the level of \
             every `size` of them.",
        ),
    },
    ParamDecl {
        group: "audio",
        name: "size",
        spec: ParamSpec::Int { default: 1024, min: 1, max: 1_000_000 },
        expression: None,
        doc: Some("Samples per window, or samples behind each level."),
    },
];
static INPUTS: &[SlotDecl] =
    &[SlotDecl { name: "input", kind: SlotType::Audio, trigger_process: true, multi: false, required: true }];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Transform],
    doc: "The audio plane crossing into the signal plane.\n\
          A stream has no frame of its own — a tap hands over whatever the last blocks held — so \
          `mode` is the framing the analysis asks for rather than the one the clock happened to \
          give: the samples as they came, a window of a fixed length, or one level per fixed run \
          of them.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(AudioIn, MANIFEST);
