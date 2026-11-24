//! Python node discovery: turn a `goofi.Node`-subclass file into an in-process node type the
//! engine hosts via `register_dyn_type`.

use std::path::Path;

use crate::{discover_one as probe_discover_one, Discovery};
use goofi_node::{Isolation, Params};
use goofi_host_sdk::{Inputs, Node, NodeCtx, NodeError, NodeResult, Outputs};

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
    tier: &'static goofi_node::IsolationCell,
) -> Box<dyn Node> {
    match PyNode::from_source(source, in_slots, out_slots) {
        Ok(n) => Box::new(n.routed_by(tier)),
        Err(e) => Box::new(FailedNode(format!("Python node construction failed: {e}"))),
    }
}

/// Probe one file for this tier, reporting all three outcomes; the [`Discovered`] it yields
/// carries the `gil_safe` flag that routes between tiers.
pub fn probe(path: &Path, ft_python: &str, memo: &Path) -> Discovery {
    probe_discover_one(path, ft_python, Isolation::InProcess, memo)
}
