//! NullTable — a table passed through whole. A fixed point to name: expressions reference it
//! with `nd()`, and the chain behind it can be rebuilt without touching the mappings.

use goofi_core::SlotType;
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, Params, SlotDecl, Tag};

#[derive(Default)]
struct NullTable;

impl Node for NullTable {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, _c: &mut NodeCtx, _p: &Params<'_>) -> NodeResult {
        let d = inp.get("input").ok_or("`input` is required")?;
        out.set("out", d.clone());
        Ok(())
    }
}

static INPUTS: &[SlotDecl] =
    &[SlotDecl { name: "input", kind: SlotType::Table, trigger_process: true, multi: false, required: true }];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Table }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Control],
    doc: "A table passed through whole, every member kept.\n\
          Name it and map params to it with `nd()`; the chain behind it can change freely.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: &[],
    producer: false,
};

goofi_signal_sdk::export!(NullTable, MANIFEST);
