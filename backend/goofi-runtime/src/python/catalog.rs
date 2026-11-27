//! How a Python node the library read is built: on the tier its type's cell names.

/// The interpreter this build embeds, which runs a node in-process; none without one.
#[cfg(feature = "embed")]
pub fn free_threaded() -> Option<String> {
    crate::python::inproc::interpreter_path()
}

#[cfg(not(feature = "embed"))]
pub fn free_threaded() -> Option<String> {
    None
}

/// The factory of a Python type and its tier cell. The cell decides the tier at every build, so
/// the runtime GIL tripwire demoting it is all a re-route takes.
pub fn factory(
    hub: std::sync::Arc<goofi_transport::Hub>,
    manifest: &'static goofi_core::node::NodeManifest,
    source: String,
    interpreter: String,
    gil_safe: bool,
) -> (goofi_sdk::node::NodeFactory, &'static goofi_core::node::IsolationCell) {
    let in_process = gil_safe && cfg!(feature = "embed");
    let tier = goofi_core::node::IsolationCell::leak(if in_process { goofi_core::node::Isolation::InProcess } else { goofi_core::node::Isolation::Subprocess });
    let in_slots = crate::loaded::in_slots(manifest);
    #[cfg(feature = "embed")]
    let out_slots: Vec<&'static str> = manifest.outputs.iter().map(|o| o.name).collect();
    let factory: goofi_sdk::node::NodeFactory = Box::new(move |_p| match tier.get() {
        #[cfg(feature = "embed")]
        goofi_core::node::Isolation::InProcess => crate::python::inproc::build_routed(&source, in_slots.clone(), out_slots.clone(), tier),
        _ => Box::new(crate::python::subproc::RemoteNode::new(crate::python::subproc::subproc(hub.clone(), &interpreter, &source), in_slots.clone()))
            as Box<dyn goofi_sdk::node::Node>,
    });
    (factory, tier)
}
