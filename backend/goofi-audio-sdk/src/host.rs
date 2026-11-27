//! The host half of the boundary: a built node's vtable behind the same [`AudioNode`] the engine
//! runs everything else as. A panic the shim caught comes back as a Rust panic at the next
//! `process`, whatever entry it was caught in, so the runtime's one catch around `process` sees a
//! loaded node and a built-in one alike.

use std::cell::RefCell;
use std::ffi::c_void;

use goofi_node::NodeManifest;

use crate::abi::{collect, BlockDesc, OutDesc, PortDesc, VTable, Write};
use crate::{AudioNode, Block, MAX_PORTS};

/// One loaded node type: its vtable and the manifest the host leaked from `goofi_describe`.
pub struct Loaded {
    vtable: &'static VTable,
    pub manifest: &'static NodeManifest,
}

impl Loaded {
    /// # Safety
    /// `library` was built by [`crate::cdylib!`] at this SDK's version — its `goofi_version`
    /// was read and matched before this is called.
    pub unsafe fn open(library: &'static libloading::Library, manifest: &'static NodeManifest) -> Result<Loaded, String> {
        Ok(Loaded { vtable: goofi_build::vtable::<VTable>(library, c"goofi_audio_node")?, manifest })
    }

    pub fn instantiate(&self) -> Box<dyn AudioNode> {
        let node = unsafe { (self.vtable.create)() };
        let panicked = node.is_null().then(|| "the constructor panicked".to_string());
        Box::new(Handle { node, vtable: self.vtable, panicked: RefCell::new(panicked) })
    }
}

struct Handle {
    node: *mut c_void,
    vtable: &'static VTable,
    /// What an entry other than `process` panicked with; the next `process` raises it.
    panicked: RefCell<Option<String>>,
}

// The instance is used from one thread at a time, as every node is: the control thread until
// it is inserted, the audio thread after.
unsafe impl Send for Handle {}

const NONE: PortDesc = PortDesc { data: std::ptr::null(), channels: 0, wired: false };

impl Handle {
    /// One entry: what it wrote, or the panic it was caught in. Nothing here allocates unless
    /// the node wrote or panicked.
    fn call(&self, entry: impl FnOnce(*mut c_void, *mut c_void, Write) -> bool) -> Result<Vec<u8>, String> {
        let mut sink: Vec<u8> = Vec::new();
        match entry(self.node, &mut sink as *mut Vec<u8> as *mut c_void, collect) {
            true => Ok(sink),
            false => Err(String::from_utf8_lossy(&sink).into_owned()),
        }
    }

    /// Call entry `name` on a live node; a panic is kept for the next `process` to raise.
    fn entry(&self, name: &str, entry: impl FnOnce(*mut c_void, *mut c_void, Write) -> bool) -> Vec<u8> {
        if self.node.is_null() {
            return Vec::new();
        }
        self.call(entry).unwrap_or_else(|text| {
            *self.panicked.borrow_mut() = Some(format!("{name}: {text}"));
            Vec::new()
        })
    }
}

impl AudioNode for Handle {
    fn channels(&self, ins: &[u16], params: &[f64], outs: usize) -> Vec<u16> {
        let mut out = vec![1u16; outs];
        self.entry("channels", |node, sink, write| unsafe {
            (self.vtable.channels)(node, ins.as_ptr(), ins.len(), params.as_ptr(), params.len(), out.as_mut_ptr(), outs, sink, write)
        });
        out
    }

    fn prepare(&mut self, rate: f64) {
        self.entry("prepare", |node, sink, write| unsafe { (self.vtable.prepare)(node, rate, sink, write) });
    }

    fn process(&mut self, b: &mut Block<'_>) {
        if let Some(text) = self.panicked.borrow_mut().take() {
            std::panic::resume_unwind(Box::new(text));
        }
        let ins: [PortDesc; MAX_PORTS] = std::array::from_fn(|i| b.ins.get(i).map_or(NONE, PortDesc::of));
        let params: [PortDesc; MAX_PORTS] = std::array::from_fn(|i| b.params.get(i).map_or(NONE, PortDesc::of));
        let outs: [OutDesc; MAX_PORTS] = std::array::from_fn(|i| match b.outs.get_mut(i) {
            Some(o) => OutDesc::of(o),
            None => OutDesc { data: std::ptr::null_mut(), channels: 0 },
        });
        let desc = BlockDesc {
            ins: ins.as_ptr(),
            n_ins: b.ins.len(),
            outs: outs.as_ptr(),
            n_outs: b.outs.len(),
            params: params.as_ptr(),
            n_params: b.params.len(),
            scalars: b.scalars.as_ptr(),
            n_scalars: b.scalars.len(),
        };
        if let Err(text) = self.call(|node, sink, write| unsafe { (self.vtable.process)(node, &desc, sink, write) }) {
            std::panic::resume_unwind(Box::new(text));
        }
    }

    fn feedback(&self) -> bool {
        let mut answer = false;
        self.entry("feedback", |node, sink, write| unsafe { (self.vtable.feedback)(node, &mut answer, sink, write) });
        answer
    }

    fn save(&self) -> Vec<u8> {
        self.entry("save", |node, sink, write| unsafe { (self.vtable.save)(node, sink, write) })
    }

    fn load(&mut self, bytes: &[u8]) {
        self.entry("load", |node, sink, write| unsafe { (self.vtable.load)(node, bytes.as_ptr(), bytes.len(), sink, write) });
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { (self.vtable.destroy)(self.node) }
    }
}
