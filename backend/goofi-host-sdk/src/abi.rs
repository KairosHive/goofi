//! The C boundary a built host node crosses: one vtable of `extern "C"` entries over codec
//! bytes, the shim that puts an author's [`Node`] behind it, and the two macros a node file and
//! its generated crate spell. Only code and plain data cross; never a Rust type.

use std::ffi::{c_char, c_void};
use std::panic::{catch_unwind, AssertUnwindSafe};

use goofi_codec::rpc::{Emitted, Request};
use goofi_codec::Out;
use goofi_core::Data;
use goofi_node::{ParamKey, Params};
use indexmap::IndexMap;

use crate::{Inputs, Manifest, Node, NodeCtx, Outputs};

pub use goofi_node::abi::{collect, version, Bytes, Segments, Write};

/// What a node may ask its runtime, as plain data.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Ctx {
    pub now: f64,
}

/// Every entry has one shape: a request in, a reply out through the host's sink.
pub type Call = unsafe extern "C" fn(node: *mut c_void, ctx: Ctx, request: Segments, sink: *mut c_void, write: Write);

#[repr(C)]
pub struct VTable {
    pub create: unsafe extern "C" fn() -> *mut c_void,
    pub destroy: unsafe extern "C" fn(node: *mut c_void),
    pub setup: Call,
    pub process: Call,
    pub on_param_changed: Call,
    pub on_param_refreshed: Call,
    pub on_pulse: Call,
}

/// The `goofi_describe` answer: the manifest as the probe schema — the one schema every
/// out-of-crate node answers.
pub fn describe_c(m: &Manifest) -> *const c_char {
    goofi_node::abi::describe_once(|| goofi_node::describe(m.tags, m.doc, m.inputs, m.outputs, m.params, m.producer))
}

/// The instance behind a `*mut c_void`: the author's node and what the shim keeps around it.
pub struct Instance {
    node: Box<dyn Node>,
    manifest: &'static Manifest,
    ctx: NodeCtx,
}

/// Box a fresh node for the host; a constructor that panics answers null, which the host
/// reports as the node's setup error rather than unwinding across the boundary.
pub fn instance(create: impl FnOnce() -> Box<dyn Node>, manifest: &'static Manifest) -> *mut c_void {
    match catch_unwind(AssertUnwindSafe(create)) {
        Ok(node) => Box::into_raw(Box::new(Instance { node, manifest, ctx: NodeCtx::new() })) as *mut c_void,
        Err(_) => std::ptr::null_mut(),
    }
}

/// # Safety
/// `node` came from [`instance`] and is not used after this.
pub unsafe extern "C" fn destroy(node: *mut c_void) {
    if !node.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| drop(Box::from_raw(node as *mut Instance))));
    }
}

/// What an entry answers, encoded onto the host's sink only once the whole call succeeded.
enum Answer {
    Done,
    Process { outputs: IndexMap<&'static str, Option<Data>>, inputs: Vec<(&'static str, usize, Data)>, clears: Vec<String> },
    Options(Option<Vec<String>>),
}

/// The host's sink as a codec sink: each run the node writes lands in the host's own buffer.
struct Sink {
    sink: *mut c_void,
    write: Write,
}

impl<'a> Out<'a> for Sink {
    fn put(&mut self, bytes: &[u8]) {
        // SAFETY: the host handed `sink` and `write` for this call, and `bytes` is a live slice.
        unsafe { (self.write)(self.sink, Bytes::of(bytes)) }
    }
}

/// Every entry the same way: decode, call, encode — a refusal or a panic is an error reply.
unsafe fn call(
    node: *mut c_void,
    ctx: Option<Ctx>,
    request: Segments,
    sink: *mut c_void,
    write: Write,
    f: impl FnOnce(&mut Instance, &[&[u8]]) -> Result<Answer, String>,
) {
    let inst = &mut *(node as *mut Instance);
    if let Some(ctx) = ctx {
        inst.ctx.now = ctx.now;
    }
    let request = request.as_slices();
    let mut out = Sink { sink, write };
    // An encoder decides before its first byte, so a refused answer leaves the sink for the error.
    let encoded = match catch_unwind(AssertUnwindSafe(|| f(inst, &request))) {
        Ok(Err(e)) => Err(e),
        Err(p) => Err(goofi_node::panic_message(p)),
        Ok(Ok(Answer::Done)) => goofi_codec::rpc::encode_response(&[], &[], &mut out).map_err(|e| e.to_string()),
        Ok(Ok(Answer::Process { outputs, inputs, clears })) => {
            // An output that IS an input crosses as that input's name: the host still holds it.
            let emitted: Vec<(&str, Emitted<'_>)> = outputs
                .iter()
                .filter_map(|(name, d)| d.as_ref().map(|d| (*name, d)))
                .map(|(name, d)| match inputs.iter().find(|(_, _, i)| i.same(d)) {
                    Some((slot, index, _)) => (name, Emitted::Input { slot, index: *index }),
                    None => (name, Emitted::Frame(d)),
                })
                .collect();
            goofi_codec::rpc::encode_response(&emitted, &clears, &mut out).map_err(|e| e.to_string())
        }
        Ok(Ok(Answer::Options(options))) => {
            goofi_codec::rpc::encode_options_response(&options).map(|bytes| out.put(&bytes)).map_err(|e| e.to_string())
        }
    };
    if let Err(e) = encoded {
        out.put(&goofi_codec::rpc::encode_error_response(&e));
    }
}

fn process_request(req: &[&[u8]]) -> Result<(goofi_codec::rpc::ParamMap, goofi_codec::rpc::SourcedSlots), String> {
    match goofi_codec::rpc::decode_request(req)? {
        Request::Process { params, slots } => Ok((params, slots)),
        Request::Refresh { .. } | Request::Pulse { .. } => Err("a refresh or a pulse where a run was expected".into()),
    }
}

/// # Safety
/// `node` came from [`instance`]; `request` and `sink` are the host's for the call.
pub unsafe extern "C" fn setup(node: *mut c_void, ctx: Ctx, request: Segments, sink: *mut c_void, write: Write) {
    call(node, Some(ctx), request, sink, write, |inst, req| {
        let (params, _) = process_request(req)?;
        inst.node.setup(&mut inst.ctx, &Params::new(&params)).map_err(|e| e.0)?;
        Ok(Answer::Done)
    })
}

/// # Safety
/// As [`setup`].
pub unsafe extern "C" fn process(node: *mut c_void, ctx: Ctx, request: Segments, sink: *mut c_void, write: Write) {
    call(node, Some(ctx), request, sink, write, |inst, req| {
        let (params, slots) = process_request(req)?;
        let mut singles: IndexMap<&'static str, Option<Data>> =
            inst.manifest.inputs.iter().filter(|s| !s.multi).map(|s| (s.name, None)).collect();
        let mut multis: crate::MultiFrames =
            inst.manifest.inputs.iter().filter(|s| s.multi).map(|s| (s.name, Vec::new())).collect();
        for (name, source, data) in slots {
            if let Some(frames) = multis.get_mut(name.as_str()) {
                frames.push((source, data));
            } else if let Some(slot) = singles.get_mut(name.as_str()) {
                *slot = Some(data);
            }
        }
        let mut outputs: IndexMap<&'static str, Option<Data>> =
            inst.manifest.outputs.iter().map(|o| (o.name, None)).collect();
        let inputs = Inputs::with_multi(&singles, &multis);
        let mut out = Outputs::new(&mut outputs);
        inst.ctx.take_cleared_inputs();
        inst.node.process(&inputs, &mut out, &mut inst.ctx, &Params::new(&params)).map_err(|e| e.0)?;
        let inputs = singles
            .iter()
            .filter_map(|(slot, d)| d.clone().map(|d| (*slot, 0, d)))
            .chain(multis.iter().flat_map(|(slot, frames)| frames.iter().enumerate().map(|(i, (_, d))| (*slot, i, d.clone()))))
            .collect();
        Ok(Answer::Process { outputs, inputs, clears: inst.ctx.take_cleared_inputs() })
    })
}

/// # Safety
/// As [`setup`]; `request` is `(group, name, Param)` as msgpack.
pub unsafe extern "C" fn on_param_changed(node: *mut c_void, ctx: Ctx, request: Segments, sink: *mut c_void, write: Write) {
    let _ = ctx;
    call(node, None, request, sink, write, |inst, req| {
        let (group, name, value): (String, String, goofi_core::Param) =
            rmp_serde::from_slice(req.concat().as_slice()).map_err(|e| e.to_string())?;
        inst.node.on_param_changed(&ParamKey::new(group, name), &value).map_err(|e| e.0)?;
        Ok(Answer::Done)
    })
}

/// # Safety
/// As [`setup`]; `request` is a codec refresh request.
pub unsafe extern "C" fn on_param_refreshed(node: *mut c_void, ctx: Ctx, request: Segments, sink: *mut c_void, write: Write) {
    let _ = ctx;
    call(node, None, request, sink, write, |inst, req| {
        let Request::Refresh { params, group, name } = goofi_codec::rpc::decode_request(req)? else {
            return Err("a run where a refresh was expected".into());
        };
        Ok(Answer::Options(inst.node.on_param_refreshed(&ParamKey::new(group, name), &Params::new(&params))))
    })
}

/// # Safety
/// As [`setup`]; `request` is a codec pulse request.
pub unsafe extern "C" fn on_pulse(node: *mut c_void, ctx: Ctx, request: Segments, sink: *mut c_void, write: Write) {
    let _ = ctx;
    call(node, None, request, sink, write, |inst, req| {
        let Request::Pulse { params, group, name } = goofi_codec::rpc::decode_request(req)? else {
            return Err("a run where a pulse was expected".into());
        };
        inst.node.on_pulse(&ParamKey::new(group, name), &Params::new(&params)).map_err(|e| e.0)?;
        Ok(Answer::Done)
    })
}

/// What a node file spells once: the type that implements [`Node`] and the manifest it declares.
#[macro_export]
macro_rules! export {
    ($node:ty, $manifest:expr) => {
        #[doc(hidden)]
        pub fn __goofi_create() -> Box<dyn $crate::Node> {
            Box::new(<$node as Default>::default())
        }
        #[doc(hidden)]
        pub static __GOOFI_MANIFEST: &$crate::Manifest = &$manifest;
    };
}

/// What the generated crate spells around the node module: the three symbols the loader reads.
/// `$sdk` is the hash of the SDK sources the crate was generated against.
#[macro_export]
macro_rules! cdylib {
    ($node:ident, $sdk:literal) => {
        #[no_mangle]
        pub extern "C" fn goofi_version() -> *const ::std::ffi::c_char {
            $crate::abi::version($sdk)
        }
        #[no_mangle]
        pub extern "C" fn goofi_describe() -> *const ::std::ffi::c_char {
            $crate::abi::describe_c($node::__GOOFI_MANIFEST)
        }
        unsafe extern "C" fn __goofi_create_raw() -> *mut ::std::ffi::c_void {
            $crate::abi::instance($node::__goofi_create, $node::__GOOFI_MANIFEST)
        }
        static __GOOFI_VTABLE: $crate::abi::VTable = $crate::abi::VTable {
            create: __goofi_create_raw,
            destroy: $crate::abi::destroy,
            setup: $crate::abi::setup,
            process: $crate::abi::process,
            on_param_changed: $crate::abi::on_param_changed,
            on_param_refreshed: $crate::abi::on_param_refreshed,
            on_pulse: $crate::abi::on_pulse,
        };
        #[no_mangle]
        pub extern "C" fn goofi_host_node() -> *const $crate::abi::VTable {
            &__GOOFI_VTABLE
        }
    };
}
