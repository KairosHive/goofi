use goofi_core::texture::{PixelFormat, Pixels, Texture};
use goofi_core::{Data, Meta, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, Params};

#[derive(Default)]
struct TextureHost;
impl Node for TextureHost {
    fn process(&mut self, _: &Inputs<'_>, out: &mut Outputs<'_>, _: &mut NodeCtx, _: &Params<'_>) -> NodeResult {
        let pixels = Pixels { width: 2, height: 2, stride: 8, format: PixelFormat::Rgb8,
            bytes: vec![255, 0, 0, 0, 255, 0, 99, 99, 0, 0, 255, 255, 255, 255, 99, 99] };
        out.set("out", Data::texture(Texture::Pixels(pixels), Meta::new())?);
        Ok(())
    }
}
static MANIFEST: Manifest = Manifest {
    doc: "Padded RGB rows for graphics integration tests.", tags: &[], inputs: &[], params: &[], producer: true,
    outputs: &[OutputDecl { name: "out", kind: SlotType::Texture }],
};
goofi_signal_sdk::export!(TextureHost, MANIFEST);
