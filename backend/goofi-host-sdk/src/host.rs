//! The host half of the boundary: a built node's vtable behind the same [`Node`] the engine runs
//! everything else as, marshalled exactly as the subprocess tier is.

use std::ffi::c_void;

use goofi_codec::Response;
use goofi_core::Data;
use goofi_node::{NodeManifest, ParamKey, Params};

use crate::abi::{collect, Bytes, Call, Ctx, VTable};
use crate::{Inputs, Node, NodeCtx, NodeError, NodeResult, Outputs};

/// One loaded node type: its vtable and the manifest the host leaked from `goofi_describe`.
pub struct Loaded {
    vtable: &'static VTable,
    manifest: &'static NodeManifest,
}

/// Which entry of the vtable a request is for — the byte a hosted child reads it as.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Setup = 0,
    Process = 1,
    ParamChanged = 2,
    Refresh = 3,
    Pulse = 4,
}

impl Entry {
    pub fn from_u8(byte: u8) -> Option<Entry> {
        [Entry::Setup, Entry::Process, Entry::ParamChanged, Entry::Refresh, Entry::Pulse].into_iter().find(|e| *e as u8 == byte)
    }
}

impl Loaded {
    /// # Safety
    /// `library` was built by [`crate::cdylib!`] at this SDK's version — its `goofi_version`
    /// was read and matched before this is called.
    pub unsafe fn open(library: &'static libloading::Library, manifest: &'static NodeManifest) -> Result<Loaded, String> {
        Ok(Loaded { vtable: goofi_build::vtable::<VTable>(library, c"goofi_host_node")?, manifest })
    }

    pub fn manifest(&self) -> &'static NodeManifest {
        self.manifest
    }

    pub fn instantiate(&self) -> Box<dyn Node> {
        Box::new(Handle { raw: self.raw(), manifest: self.manifest })
    }

    /// A fresh instance called by entry, request bytes in and reply bytes out — what a hosted
    /// child forwards across its exchange.
    pub fn raw(&self) -> Raw {
        Raw { node: unsafe { (self.vtable.create)() }, vtable: self.vtable }
    }
}

/// One instance behind the vtable, spoken to in codec bytes.
pub struct Raw {
    node: *mut c_void,
    vtable: &'static VTable,
}

// The instance is used from the one thread that runs it, as every node is.
unsafe impl Send for Raw {}

impl Raw {
    pub fn call(&mut self, entry: Entry, now: f64, request: &[u8]) -> Result<Vec<u8>, String> {
        if self.node.is_null() {
            return Err("the node's constructor panicked".into());
        }
        let call: Call = match entry {
            Entry::Setup => self.vtable.setup,
            Entry::Process => self.vtable.process,
            Entry::ParamChanged => self.vtable.on_param_changed,
            Entry::Refresh => self.vtable.on_param_refreshed,
            Entry::Pulse => self.vtable.on_pulse,
        };
        let mut reply: Vec<u8> = Vec::new();
        // SAFETY: the entry is the node's own, `request` and the sink live for the call.
        unsafe { call(self.node, Ctx { now }, Bytes::of(request), &mut reply as *mut Vec<u8> as *mut c_void, collect) };
        Ok(reply)
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        unsafe { (self.vtable.destroy)(self.node) }
    }
}

struct Handle {
    raw: Raw,
    manifest: &'static NodeManifest,
}

impl Handle {
    fn call(&mut self, entry: Entry, now: f64, request: &[u8]) -> Result<Response, String> {
        goofi_codec::decode_response(&self.raw.call(entry, now, request)?)
    }

    fn done(answer: Result<Response, String>) -> NodeResult {
        match answer {
            Ok(Response::Process(_)) => Ok(()),
            Ok(Response::NodeError(msg)) => Err(NodeError(msg)),
            Ok(Response::Options(_)) => Err(NodeError("the node answered options where none were asked".into())),
            Err(e) => Err(NodeError(e)),
        }
    }
}

impl Node for Handle {
    fn setup(&mut self, ctx: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        Self::done(self.call(Entry::Setup, ctx.now, &goofi_codec::encode_request(p.groups(), &[])))
    }

    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, ctx: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        // Every present frame crosses, a `multi` slot's under its one name repeated, each with
        // its source; a single slot's has none.
        let mut present: Vec<(&str, &str, &Data)> = Vec::new();
        for slot in self.manifest.inputs {
            if slot.multi {
                present.extend(inp.get_multi(slot.name).iter().map(|(source, d)| (slot.name, source.as_str(), d)));
            } else if let Some(d) = inp.get(slot.name) {
                present.push((slot.name, "", d));
            }
        }
        match self.call(Entry::Process, ctx.now, &goofi_codec::encode_request(p.groups(), &present)) {
            Ok(Response::Process(result)) => {
                for slot in result.clear_inputs {
                    ctx.clear_input(&slot);
                }
                for (slot, data) in result.outputs {
                    out.set(&slot, data);
                }
                Ok(())
            }
            other => Self::done(other),
        }
    }

    fn on_param_changed(&mut self, key: &ParamKey, v: &goofi_core::Param) -> NodeResult {
        let request = rmp_serde::to_vec(&(&key.group, &key.name, v)).map_err(|e| NodeError(e.to_string()))?;
        Self::done(self.call(Entry::ParamChanged, 0.0, &request))
    }

    fn on_param_refreshed(&mut self, key: &ParamKey, p: &Params<'_>) -> Option<Vec<String>> {
        let request = goofi_codec::encode_refresh_request(p.groups(), &key.group, &key.name);
        match self.call(Entry::Refresh, 0.0, &request) {
            Ok(Response::Options(options)) => options,
            _ => None,
        }
    }

    fn on_pulse(&mut self, key: &ParamKey, p: &Params<'_>) -> NodeResult {
        let request = goofi_codec::encode_pulse_request(p.groups(), &key.group, &key.name);
        Self::done(self.call(Entry::Pulse, 0.0, &request))
    }
}
