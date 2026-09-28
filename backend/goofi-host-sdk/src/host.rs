//! The host half of the boundary: a built node's vtable behind the same [`Node`] the engine runs
//! everything else as, marshalled exactly as the subprocess tier is.

use std::ffi::c_void;

use goofi_codec::{Output, ProcessOutput, Response};
use goofi_core::Data;
use goofi_node::{NodeManifest, ParamKey, Params};

use crate::abi::{collect, Bytes, Call, Ctx, Segments, VTable};
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
        Box::new(Handle::new(self.raw(), self.manifest))
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

/// Whoever answers an entry in codec bytes: the vtable itself, or a child that holds it.
pub trait Ask: Send {
    fn ask(&mut self, entry: Entry, now: f64, request: &[&[u8]]) -> Result<Vec<u8>, String>;
}

impl Ask for Raw {
    fn ask(&mut self, entry: Entry, now: f64, request: &[&[u8]]) -> Result<Vec<u8>, String> {
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
        let runs: Vec<Bytes> = request.iter().map(|r| Bytes::of(r)).collect();
        let mut reply: Vec<u8> = Vec::new();
        // SAFETY: the entry is the node's own, `request` and the sink live for the call.
        unsafe { call(self.node, Ctx { now }, Segments::of(&runs), &mut reply as *mut Vec<u8> as *mut c_void, collect) };
        Ok(reply)
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        unsafe { (self.vtable.destroy)(self.node) }
    }
}

/// A node behind an [`Ask`]: the `Node` the engine runs, marshalled as the subprocess tier is.
pub struct Handle<A: Ask> {
    ask: A,
    manifest: &'static NodeManifest,
}

impl<A: Ask> Handle<A> {
    pub fn new(ask: A, manifest: &'static NodeManifest) -> Handle<A> {
        Handle { ask, manifest }
    }

    fn call(&mut self, entry: Entry, now: f64, request: &[&[u8]]) -> Result<Response, String> {
        goofi_codec::decode_response(self.ask.ask(entry, now, request)?)
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

impl<A: Ask> Node for Handle<A> {
    fn setup(&mut self, ctx: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let request = goofi_codec::encode_request(p.groups(), &[]);
        Self::done(self.call(Entry::Setup, ctx.now, &runs(&request)))
    }

    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, ctx: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let present = present(self.manifest.inputs.iter().map(|s| (s.name, s.multi)), inp);
        let request = goofi_codec::encode_request(p.groups(), &present);
        match self.call(Entry::Process, ctx.now, &runs(&request)) {
            Ok(Response::Process(result)) => apply(result, inp, out, ctx),
            other => Self::done(other),
        }
    }

    fn on_param_changed(&mut self, key: &ParamKey, v: &goofi_core::Param) -> NodeResult {
        let request = rmp_serde::to_vec(&(&key.group, &key.name, v)).map_err(|e| NodeError(e.to_string()))?;
        Self::done(self.call(Entry::ParamChanged, 0.0, &[&request]))
    }

    fn on_param_refreshed(&mut self, key: &ParamKey, p: &Params<'_>) -> Option<Vec<String>> {
        let request = goofi_codec::encode_refresh_request(p.groups(), &key.group, &key.name);
        match self.call(Entry::Refresh, 0.0, &[&request]) {
            Ok(Response::Options(options)) => options,
            _ => None,
        }
    }

    fn on_pulse(&mut self, key: &ParamKey, p: &Params<'_>) -> NodeResult {
        let request = goofi_codec::encode_pulse_request(p.groups(), &key.group, &key.name);
        Self::done(self.call(Entry::Pulse, 0.0, &[&request]))
    }
}

/// A request's runs as the slices an [`Ask`] takes.
pub fn runs<'a>(request: &'a goofi_codec::Runs<'_>) -> Vec<&'a [u8]> {
    request.iter().map(|r| &**r).collect()
}

/// Every present frame, a `multi` slot's under its one name repeated, each with its source; a
/// single slot's has none. `slots` is each declared input with whether it is `multi`.
pub fn present<'a>(slots: impl Iterator<Item = (&'a str, bool)>, inp: &'a Inputs<'_>) -> Vec<(&'a str, &'a str, &'a Data)> {
    let mut present = Vec::new();
    for (name, multi) in slots {
        if multi {
            present.extend(inp.get_multi(name).iter().map(|(source, d)| (name, source.as_str(), d)));
        } else if let Some(d) = inp.get(name) {
            present.push((name, "", d));
        }
    }
    present
}

/// Land a call's outputs and clears: an output that names an input is that input's own frame.
pub fn apply(result: ProcessOutput, inp: &Inputs<'_>, out: &mut Outputs<'_>, ctx: &mut NodeCtx) -> NodeResult {
    for slot in result.clear_inputs {
        ctx.clear_input(&slot);
    }
    for (slot, output) in result.outputs {
        let data = match output {
            Output::Frame(data) => data,
            Output::Input { slot: input, index } => inp
                .at(&input, index)
                .cloned()
                .ok_or_else(|| NodeError(format!("output `{slot}` names input `{input}`, which holds no frame")))?,
        };
        out.set(&slot, data);
    }
    Ok(())
}
