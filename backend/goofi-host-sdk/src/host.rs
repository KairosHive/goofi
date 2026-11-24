//! The host half of the boundary: whatever answers a call in codec bytes — a vtable in this
//! process, or a child that holds one — behind the same [`Node`] the engine runs everything as.

use std::ffi::c_void;

use goofi_codec::rpc::{self, Entry, Output, ProcessOutput, Response};
use goofi_core::Data;
use goofi_node::{NodeManifest, ParamKey, Params};

use crate::abi::{self, collect, Bytes, Ctx, Segments, VTable};
use crate::{Inputs, Node, NodeCtx, NodeError, NodeResult, Outputs};

/// One loaded node type: its vtable and the manifest the host leaked from `goofi_describe`.
pub struct Loaded {
    vtable: &'static VTable,
    manifest: &'static NodeManifest,
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
        Box::new(CodecNode::new(self.raw(), in_slots(self.manifest)))
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

/// Whoever answers an entry in codec bytes: the vtable itself, or a child that holds it. The
/// one child RPC: every implementation takes `[entry][now]` and the request's runs.
pub trait Call: Send {
    fn call(&mut self, entry: Entry, now: f64, request: &[&[u8]]) -> Result<Vec<u8>, String>;
    /// Whether the answerer holds no params yet — a child not running — so the next call must be
    /// a setup carrying the whole map. False for the vtable, which lives as long as the node.
    fn needs_seed(&mut self) -> bool {
        false
    }
}

impl Call for Raw {
    fn call(&mut self, entry: Entry, now: f64, request: &[&[u8]]) -> Result<Vec<u8>, String> {
        if self.node.is_null() {
            return Err("the node's constructor panicked".into());
        }
        let call: abi::Call = match entry {
            Entry::Setup => self.vtable.setup,
            Entry::Process => self.vtable.process,
            Entry::ParamChanged => self.vtable.on_param_changed,
            Entry::Refresh => self.vtable.on_param_refreshed,
            Entry::Pulse => self.vtable.on_pulse,
            // The drop destroys: an in-process node has no child to tell.
            Entry::Stop => return Ok(rpc::done()),
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

/// Each declared input slot with whether it is `multi`: what a call's present frames are named by.
pub fn in_slots(manifest: &NodeManifest) -> Vec<(&'static str, bool)> {
    manifest.inputs.iter().map(|s| (s.name, s.multi)).collect()
}

/// A node behind a [`Call`]: the `Node` the engine runs, marshalled in codec bytes. Its drop
/// sends `Stop`, so a child releases what it holds before it goes.
pub struct CodecNode<C: Call> {
    call: C,
    in_slots: Vec<(&'static str, bool)>,
    /// Whether the answerer's setup succeeded; a refused one is asked again before the next run.
    seeded: bool,
}

impl<C: Call> CodecNode<C> {
    pub fn new(call: C, in_slots: Vec<(&'static str, bool)>) -> CodecNode<C> {
        CodecNode { call, in_slots, seeded: false }
    }

    fn call(&mut self, entry: Entry, now: f64, request: &[&[u8]]) -> Result<Response, String> {
        goofi_codec::rpc::decode_response(self.call.call(entry, now, request)?)
    }

    /// A child that is not running knows no params: it is seeded with the whole map before it is
    /// asked anything else, which also re-seeds one that replaced a failed child.
    fn seed(&mut self, now: f64, p: &Params<'_>) -> Result<(), NodeError> {
        if self.seeded && !self.call.needs_seed() {
            return Ok(());
        }
        self.setup_with(now, p)
    }

    fn setup_with(&mut self, now: f64, p: &Params<'_>) -> NodeResult {
        let request = goofi_codec::rpc::encode_setup_request(p.groups()).map_err(|e| NodeError(e.to_string()))?;
        let done = Self::done(self.call(Entry::Setup, now, &[&request]));
        self.seeded = done.is_ok();
        done
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

impl<C: Call> Drop for CodecNode<C> {
    fn drop(&mut self) {
        let _ = self.call.call(Entry::Stop, 0.0, &[]);
    }
}

impl<C: Call> Node for CodecNode<C> {
    fn setup(&mut self, ctx: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        self.setup_with(ctx.now, p)
    }

    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, ctx: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        self.seed(ctx.now, p)?;
        let present = present(self.in_slots.iter().copied(), inp);
        let request = goofi_codec::rpc::encode_request(&present).map_err(|e| NodeError(e.to_string()))?;
        match self.call(Entry::Process, ctx.now, &request.iter().map(|r| &**r).collect::<Vec<_>>()) {
            Ok(Response::Process(result)) => apply(result, inp, out, ctx),
            other => Self::done(other),
        }
    }

    /// One moved param. A child not running hears it with the whole map, at its next seed.
    fn on_param_changed(&mut self, key: &ParamKey, v: &goofi_core::Param) -> NodeResult {
        if !self.seeded || self.call.needs_seed() {
            return Ok(());
        }
        let request = goofi_codec::rpc::encode_param_request(&key.group, &key.name, v).map_err(|e| NodeError(e.to_string()))?;
        Self::done(self.call(Entry::ParamChanged, 0.0, &[&request]))
    }

    fn on_param_refreshed(&mut self, key: &ParamKey, p: &Params<'_>) -> Option<Vec<String>> {
        self.seed(0.0, p).ok()?;
        let request = goofi_codec::rpc::encode_refresh_request(&key.group, &key.name).ok()?;
        match self.call(Entry::Refresh, 0.0, &[&request]) {
            Ok(Response::Options(options)) => options,
            _ => None,
        }
    }

    fn on_pulse(&mut self, key: &ParamKey, p: &Params<'_>) -> NodeResult {
        self.seed(0.0, p)?;
        let request = goofi_codec::rpc::encode_pulse_request(&key.group, &key.name).map_err(|e| NodeError(e.to_string()))?;
        Self::done(self.call(Entry::Pulse, 0.0, &[&request]))
    }
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
