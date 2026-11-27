//! A Python node file's source as an in-process node the engine hosts.

use goofi_core::node::Params;
use goofi_sdk::node::{Inputs, Node, NodeCtx, NodeError, NodeResult, Outputs};

use super::PyNode;

/// A stand-in for a node whose construction failed; it reports the error from `setup()`.
struct FailedNode(String);

impl Node for FailedNode {
    fn setup(&mut self, _ctx: &mut NodeCtx, _p: &Params<'_>) -> NodeResult {
        Err(NodeError(self.0.clone()))
    }
    fn process(
        &mut self,
        _inp: &Inputs<'_>,
        _out: &mut Outputs<'_>,
        _ctx: &mut NodeCtx,
        _p: &Params<'_>,
    ) -> NodeResult {
        Err(NodeError(self.0.clone()))
    }
}

/// Build a [`PyNode`] wired to demote its own type when the GIL tripwire fires, or a
/// [`FailedNode`] — never a panic, because this runs under the graph mutex.
pub fn build_routed(
    source: &str,
    in_slots: Vec<(&'static str, bool)>,
    out_slots: Vec<&'static str>,
    tier: &'static goofi_core::node::IsolationCell,
) -> Box<dyn Node> {
    match PyNode::from_source(source, in_slots, out_slots) {
        Ok(n) => Box::new(n.routed_by(tier)),
        Err(e) => Box::new(FailedNode(format!("Python node construction failed: {e}"))),
    }
}
