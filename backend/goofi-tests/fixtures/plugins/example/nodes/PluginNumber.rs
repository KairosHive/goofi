//! A bundled native source used by the plugin session.
use goofi_core::{Data, Meta, SlotType};
use goofi_signal_sdk::{
    Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, Params, Tag,
};

#[derive(Default)]
struct PluginNumber;
impl Node for PluginNumber {
    fn process(
        &mut self,
        _: &Inputs<'_>,
        out: &mut Outputs<'_>,
        _: &mut NodeCtx,
        _: &Params<'_>,
    ) -> NodeResult {
        out.set(
            "out",
            Data::array_f32(vec![1], 7.0_f32.to_le_bytes().to_vec(), Meta::new())
                .map_err(|e| e.to_string())?,
        );
        Ok(())
    }
}
static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Generator],
    doc: "A plugin source.",
    inputs: &[],
    outputs: &[OutputDecl {
        name: "out",
        kind: SlotType::Array,
    }],
    params: &[],
    producer: true,
};
goofi_signal_sdk::export!(PluginNumber, MANIFEST);
