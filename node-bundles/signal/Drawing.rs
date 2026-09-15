//! Drawing — a drawing widget's picture, as a frame. Point `image` at the pad the way a knob's
//! value is pointed at a variable — an expression of `variables.<panel>.<pad>` — and every stroke it
//! holds arrives here as RGBA. `graphics:SignalIn` is what puts it on the GPU.

use goofi_core::{png, Data, Meta, SlotType};
use goofi_signal_sdk::{
    Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, Params, ParamSpec, Tag,
};

#[derive(Default)]
struct Drawing {
    /// The URL last read, and the frame it decoded to. A picture nobody has drawn on is published
    /// again rather than decoded again — the decode is the cost, not the publish.
    last: String,
    drawn: Option<Data>,
}

impl Node for Drawing {
    fn process(
        &mut self,
        _inp: &Inputs<'_>,
        out: &mut Outputs<'_>,
        _c: &mut NodeCtx,
        p: &Params<'_>,
    ) -> NodeResult {
        let image = p.str("drawing", "image").unwrap_or_default();
        if image != self.last {
            self.last = image.to_string();
            self.drawn = None;
            if !image.trim().is_empty() {
                let picture = png::decode(image).map_err(|e| e.to_string())?;
                // A colour frame spans 0..1, which is the range every viewer and the GPU read.
                let texels: Vec<u8> =
                    picture.rgba.iter().flat_map(|b| (*b as f32 / 255.0).to_le_bytes()).collect();
                let shape = vec![picture.height as usize, picture.width as usize, 4];
                self.drawn = Some(Data::array_f32(shape, texels, Meta::new()).map_err(|e| e.to_string())?);
            }
        }
        // An empty pad has no picture, so it emits nothing: `nothing emitted yet` is the honest
        // reading of a page nobody has drawn on.
        if let Some(frame) = &self.drawn {
            out.set("out", frame.clone());
        }
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[ParamDecl {
    group: "drawing",
    name: "image",
    spec: ParamSpec::Str { default: "", options: &[], refresh: false },
    expression: None,
    doc: Some(
        "The pad to read, as an expression of `variables.<panel>.<pad>` — the same way a knob's \
         value is pointed at a variable. It holds the drawing as a PNG data URL.",
    ),
}];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Image, Tag::Input],
    doc: "A drawing widget's picture, as a frame.\n\
          Every stroke a `paint` widget holds — a hand's or `control paint`'s — as an [H, W, 4] \
          RGBA frame spanning 0..1. Feed `graphics:SignalIn` with it to put the drawing on the GPU.",
    inputs: &[],
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(Drawing, MANIFEST);
