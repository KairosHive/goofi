//! Drawing — a drawing widget's picture, as a frame. Point `image` at the pad the way a knob's
//! value is pointed at a variable — an expression of `variables.<panel>.<pad>` — and every op it
//! holds arrives here rasterized as RGBA. `graphics:SignalIn` is what puts it on the GPU.

use goofi_core::{drawing, Data, Meta, SlotType};
use goofi_signal_sdk::{
    Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, Params, ParamSpec, Tag,
};

#[derive(Default)]
struct Drawing {
    /// The byte code and size last read, and the frame they rasterized to.
    last: (String, i64),
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
        let size = p.i64("drawing", "size").unwrap_or(512).clamp(1, 4096);
        if (image, size) != (self.last.0.as_str(), self.last.1) {
            self.last = (image.to_string(), size);
            self.drawn = None;
            if !image.trim().is_empty() {
                let ops = drawing::from_value(image)?;
                let rgba = drawing::raster(&ops, size as u32, size as u32)?;
                // A colour frame spans 0..1, which is the range every viewer and the GPU read.
                let texels: Vec<u8> = rgba.iter().flat_map(|b| (*b as f32 / 255.0).to_le_bytes()).collect();
                let shape = vec![size as usize, size as usize, 4];
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

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "drawing",
        name: "image",
        spec: ParamSpec::Str { default: "", options: &[], refresh: false },
        expression: None,
        doc: Some(
            "The pad to read, as an expression of `variables.<panel>.<pad>` — the same way a knob's \
             value is pointed at a variable. It holds the drawing as base64 byte code.",
        ),
        section: 0,
        show: None,
        role: None,
    },
    ParamDecl {
        group: "drawing",
        name: "size",
        spec: ParamSpec::Num { default: &[512.0], min: 1.0, max: 4096.0, int: true, options: &[], color: false },
        expression: None,
        doc: Some("The frame's width and height in pixels."),
        section: 0,
        show: None,
        role: None,
    },
];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Image, Tag::Input],
    doc: "A drawing widget's picture, as a frame.\n\
          Every op a `paint` widget holds — a hand's or `control paint`'s — rasterized as a \
          [size, size, 4] RGBA frame spanning 0..1. Feed `graphics:SignalIn` with it to put the \
          drawing on the GPU.",
    inputs: &[],
    outputs: OUTPUTS,
    params: PARAMS,
    producer: true,
};

goofi_signal_sdk::export!(Drawing, MANIFEST);
