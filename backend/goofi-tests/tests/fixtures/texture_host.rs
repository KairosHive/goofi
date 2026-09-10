use goofi_core::{Data, Meta, SlotType};
use goofi_graphics_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamSpec, Params, Pixels, PixelFormat, Texture};

#[derive(Default)]
struct TextureHost;
impl Node for TextureHost {
    fn process(&mut self, _: &Inputs<'_>, out: &mut Outputs<'_>, _: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let pixels = Pixels { width: 2, height: 2, stride: 8, format: PixelFormat::Rgb8,
            bytes: vec![255, 0, 0, 0, 255, 0, 99, 99, 0, 0, 255, 255, 255, 255, 99, 99] };
        let texture = match p.str("image", "mode").unwrap_or("pixels") {
            "render" => Texture::Render { width: 2, height: 2,
                source: "fn shade(uv: vec2f) -> vec4f { return vec4f(uv, 0.5, 1.0); }".into() },
            "broken" => Texture::Render { width: 2, height: 2, source: "invalid program".into() },
            _ => Texture::Pixels(pixels),
        };
        out.set("out", Data::texture(texture, Meta::new())?);
        Ok(())
    }
}
static MANIFEST: Manifest = Manifest {
    doc: "Padded RGB rows for graphics integration tests.", tags: &[], inputs: &[], params: &[ParamDecl { group: "image", name: "mode", expression: None, doc: None,
        spec: ParamSpec::Str { default: "pixels", options: &["pixels", "render", "broken"], refresh: false } }], producer: true,
    outputs: &[OutputDecl { name: "out", kind: SlotType::Texture }],
};
goofi_graphics_sdk::export!(TextureHost, MANIFEST);
