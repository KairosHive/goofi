//! Variable — a variable's texels as a texture. The named variable's wire feeds `value` as a
//! cable would, and an `[H, W, 4]` or `[H, W, 3]` frame — a pad's sheet — becomes the texture.

use goofi_core::{Data, Meta, SlotType, Value};
use goofi_graphics_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamSpec, Params, PixelFormat, Pixels, Role, SlotDecl, Tag, Texture};

#[derive(Default)]
struct Variable;

impl Node for Variable {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, _c: &mut NodeCtx, _p: &Params<'_>) -> NodeResult {
        let Some(Value::Array(a)) = inp.get("value").map(Data::value) else { return Ok(()) };
        let (h, w, c) = match *a.shape() {
            [h, w, c] if c == 3 || c == 4 => (h, w, c),
            ref shape => return Err(format!("a texture takes [H, W, 4] or [H, W, 3] texels, not {shape:?}").into()),
        };
        let format = if c == 4 { PixelFormat::Rgba32Float } else { PixelFormat::Rgb32Float };
        let bytes: Vec<u8> = a.values().flat_map(f32::to_le_bytes).collect();
        let pixels = Pixels { width: w as u32, height: h as u32, stride: w * c * 4, format, bytes };
        out.set("out", Data::texture(Texture::Pixels(pixels), Meta::new())?);
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "variable",
        name: "group",
        spec: ParamSpec::Str { default: "", options: &[], refresh: true },
        expression: None,
        doc: Some("the variable's group; the list is the patch's groups"),
        section: 0,
        show: None,
        role: None,
    },
    ParamDecl {
        group: "variable",
        name: "element",
        spec: ParamSpec::Str { default: "", options: &[], refresh: true },
        expression: None,
        doc: Some("the variable in that group; the list follows the group chosen"),
        section: 0,
        show: None,
        role: Some(Role::Feed { slot: "value", group: "group" }),
    },
];

static INPUTS: &[SlotDecl] = &[SlotDecl { name: "value", kind: SlotType::Array, trigger_process: true, multi: false, required: false }];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Texture }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Input],
    doc: "A variable's texels as a texture.\n\
          `group` and `element` pick the variable; an `[H, W, 4]` or `[H, W, 3]` frame of values in 0..1 — a \
          paint pad's sheet — is the texture, top row first.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_graphics_sdk::export!(Variable, MANIFEST);
