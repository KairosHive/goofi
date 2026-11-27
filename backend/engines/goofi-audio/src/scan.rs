//! What the audio engine registers from a node file: a Rust node built against
//! `goofi_sdk::audio`, loaded behind its version symbol. A `.vst3` bundle in a folder is `vst3`'s.

use std::path::Path;
use std::sync::Arc;

use crate::loaded::Loaded;
use goofi_core::node::{Body, Isolation, NodeFile, Read, Scanned};

use crate::nodes::Class;
use crate::AudioEngine;

impl AudioEngine {
    /// A library that will not load displaces a stale registration and greys the type out.
    pub(crate) fn register_file(&mut self, file: &NodeFile, read: &Read) -> Scanned {
        let loaded = match &read.body {
            Body::Rust { artifact, .. } => self.load_rust(artifact, file, read),
            // A block has a deadline, and no call out of this process keeps one.
            body => Err(format!("the audio engine runs Rust nodes, not {}: a block is computed in its own process", body.language())),
        };
        match loaded {
            Ok(replaced) => Scanned::Registered { isolation: Isolation::Native, replaced },
            Err(reason) => {
                self.classes.remove(file.type_name.as_str());
                Scanned::Unavailable(reason)
            }
        }
    }

    fn load_rust(&mut self, artifact: &Path, file: &NodeFile, read: &Read) -> Result<bool, String> {
        let manifest = goofi_core::node::manifest_of(&file.type_name, &read.intro)?;
        let loaded = unsafe { Loaded::open(goofi_build::open(artifact)?.library) }?;
        let class = Class { manifest, make: Arc::new(move |_| loaded.instantiate()), plugin: None };
        Ok(self.classes.insert(manifest.type_name, class).is_some())
    }
}
