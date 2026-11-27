//! What the signal engine registers from a node file: a Python node on the tier its imports
//! allow, and a Rust node that names `goofi_sdk::signal`, in this process or in a child.

use goofi_core::node::{Body, Isolation, NodeFile, Read, Scanned};

use crate::SignalEngine;

impl SignalEngine {
    /// The latest scan is the answer: a file that does not load displaces a stale registration.
    pub(crate) fn register_file(&mut self, file: &NodeFile, read: &Read) -> Scanned {
        match self.load(file, read) {
            Ok((replaced, isolation)) => Scanned::Registered { isolation, replaced },
            Err(reason) => {
                self.dyn_types.remove(file.type_name.as_str());
                Scanned::Unavailable(reason)
            }
        }
    }

    fn load(&mut self, file: &NodeFile, read: &Read) -> Result<(bool, Isolation), String> {
        let manifest = goofi_core::node::manifest_of(&file.type_name, &read.intro)?;
        let (factory, tier) = match &read.body {
            Body::Python { source, interpreter } => {
                goofi_runtime::python::catalog::factory(self.hub.clone(), manifest, source.clone(), interpreter.clone(), read.intro.gil_safe)
            }
            Body::Rust { artifact, host } => goofi_runtime::hosted::factory(self.hub.clone(), artifact.clone(), host.clone(), manifest)?,
            body => return Err(format!("the signal engine runs Python and Rust nodes, not {}", body.language())),
        };
        Ok((self.register_dyn_type(manifest, factory, tier), tier.get()))
    }
}
