//! GraphicsIn — a texture as numbers. A tap reads a frame back at the size its readers draw, and
//! nothing else in the patch asks for pixels, so the whole render lands here: `size` is what
//! keeps a 4K frame from becoming four million numbers the rest of the patch has to carry.

use goofi_core::reduce::{note_reduced, origin_of, reduce_axis, ReduceMethod};
use goofi_core::{Data, Meta, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, Params, ParamSpec, SlotDecl, Tag};

/// Rec. 709 luminance, over whatever of R, G and B the frame has.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

#[derive(Default)]
struct GraphicsIn;

fn read(bytes: &[u8], i: usize) -> f32 {
    f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("four bytes"))
}

/// One texel's brightness: its channels under the luminance weights, which sum to one, so a frame
/// of one channel crosses unchanged.
fn luma(bytes: &[u8], at: usize, c: usize) -> f32 {
    match c {
        1 => read(bytes, at),
        2 => read(bytes, at),
        _ => (0..3).map(|k| LUMA[k] * read(bytes, at + k)).sum(),
    }
}

impl Node for GraphicsIn {
    fn process(
        &mut self,
        inp: &Inputs<'_>,
        out: &mut Outputs<'_>,
        _c: &mut NodeCtx,
        p: &Params<'_>,
    ) -> NodeResult {
        let d = inp.get("input").ok_or("`input` is required")?;
        let a = d.assert_ndims().at_least(2)?;
        let mut shape = a.shape().to_vec();

        // Both picture axes to the same bound, so a shrunk frame keeps the shape it had.
        let size = p.i64("graphics", "size").unwrap_or(256).clamp(0, 16384) as usize;
        let (mut noted, mut reduced) = (Vec::new(), None::<Vec<u8>>);
        for dim in [0, 1] {
            let was = shape[dim];
            let bytes = reduced.as_deref().unwrap_or(a.as_bytes());
            let Some(cut) = reduce_axis(bytes, &shape, dim, size, ReduceMethod::Area) else { continue };
            noted.push((dim, origin_of(d.meta(), dim, was), ReduceMethod::Area));
            (reduced, shape[dim]) = (Some(cut.bytes), cut.new_len);
        }

        let (h, w) = (shape[0], shape[1]);
        let c = shape.get(2).copied().unwrap_or(1);
        let mode = p.str("graphics", "mode").unwrap_or("pixels");
        if mode == "pixels" {
            // A frame nothing cut crosses as it came, its buffer shared rather than copied.
            let frame = match reduced {
                None => d.clone(),
                Some(bytes) => {
                    let mut meta = d.meta().clone();
                    note_reduced(&mut meta, &noted);
                    Data::array_f32(shape, bytes, meta).map_err(|e| e.to_string())?
                }
            };
            out.set("out", frame);
            return Ok(());
        }
        let bytes = reduced.as_deref().unwrap_or(a.as_bytes());
        // Every reduction below is over brightness, so the channel axis is spent and its labels
        // with it; the picture axes keep the note that says what they were rendered from.
        let mut gray = Vec::with_capacity(h * w);
        for i in 0..h * w {
            gray.push(luma(bytes, i * c, c));
        }
        let (shape, values) = match mode {
            "rows" => (vec![h], (0..h).map(|y| gray[y * w..(y + 1) * w].iter().sum::<f32>() / w as f32).collect()),
            "columns" => {
                (vec![w], (0..w).map(|x| (0..h).map(|y| gray[y * w + x]).sum::<f32>() / h as f32).collect())
            }
            _ => (vec![h, w], gray),
        };
        let mut meta = Meta::new();
        note_reduced(&mut meta, &noted);
        let buf: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        out.set("out", Data::array_f32(shape, buf, meta).map_err(|e| e.to_string())?);
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "graphics",
        name: "mode",
        spec: ParamSpec::Str {
            default: "pixels",
            options: &["pixels", "gray", "rows", "columns"],
            refresh: false,
        },
        expression: None,
        doc: Some(
            "The texels as they came, their brightness, or the profile that brightness draws \
             down the frame or across it.",
        ),
    },
    ParamDecl {
        group: "graphics",
        name: "size",
        spec: ParamSpec::Int { default: 256, min: 0, max: 16384, options: &[] },
        expression: None,
        doc: Some(
            "The most texels either picture axis keeps, averaged into blocks. Zero takes the \
             frame as it was rendered, which is every texel of it.",
        ),
    },
];
static INPUTS: &[SlotDecl] =
    &[SlotDecl { name: "input", kind: SlotType::Texture, trigger_process: true, multi: false, required: true }];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Transform],
    doc: "The graphics plane crossing into the signal plane.\n\
          A render is more numbers than the rest of a patch wants to carry, so `size` bounds each \
          picture axis and the frame still says what it was rendered from. `mode` is what a \
          picture is usually wanted for: its texels, its brightness, or the profile that \
          brightness draws across the frame or down it.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(GraphicsIn, MANIFEST);
