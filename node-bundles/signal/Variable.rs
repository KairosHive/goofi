//! Variable — a variable's frame on a cable: the one bridge from the variable layer into the
//! graph. The named variable's wire feeds `value` as a cable would, and each frame leaves whole
//! on the output of its kind.

use goofi_core::{Data, Meta, SlotType, Value};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamSpec, Params, Role, SlotDecl, Tag};

#[derive(Default)]
struct Variable;

impl Node for Variable {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, _c: &mut NodeCtx, _p: &Params<'_>) -> NodeResult {
        // Re-stamped: the frame is this node's emission, not the variable's.
        match inp.get("value").map(Data::value) {
            Some(Value::Array(a)) => out.set("out", Data::array(a.clone(), Meta::new())),
            Some(Value::Str(s)) => out.set("text", Data::string(s.clone(), Meta::new())),
            _ => {}
        }
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[ParamDecl {
    group: "variable",
    name: "name",
    spec: ParamSpec::Str { default: "", options: &[], refresh: true },
    expression: None,
    doc: Some("the variable to read, `group.element`"),
    section: 0,
    show: None,
    role: Some(Role::Feed { slot: "value" }),
}];

static INPUTS: &[SlotDecl] = &[SlotDecl { name: "value", kind: SlotType::Array, trigger_process: true, multi: false, required: false }];
static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Array }, OutputDecl { name: "text", kind: SlotType::String }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Input],
    doc: "A variable's frame on a cable.\n\
          `name` picks the variable; every frame it holds leaves whole on `out`, or on `text` for a \
          string. A MIDI device's `notes` or a pad's sheet reaches a node this way.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(Variable, MANIFEST);
