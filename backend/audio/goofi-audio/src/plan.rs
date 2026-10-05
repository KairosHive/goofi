//! The plan: what the audio thread runs each block, compiled on the control thread from the
//! settled view — an order, the arena's regions, and where every port and param reads.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;

use goofi_audio_sdk::BLOCK;
use goofi_core::SlotType;
use goofi_runtime::{param_of, scalar_of};
use goofi_node::{BindingView, GraphView, NodeManifest, NodeView, Uid};

use crate::Instance;

/// An offset into the arena, in floats; a region is `channels * BLOCK` of them.
pub type Region = usize;

/// The widest a port may be: a bound on what a garbled width can ask of the arena and the rings,
/// not a design limit. A port this wide is 256 KB of arena and some 400 MB of rings.
pub const CEILING: u16 = 1024;

/// The silence every unwired input reads: one channel at the arena's start.
pub const SILENCE: Region = 0;

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Silence,
    Region { at: Region, channels: u16 },
    /// A multi input: its wires summed into a scratch region, each read through `Port::chan`'s
    /// one rule for a narrower part.
    Sum { at: Region, channels: u16, parts: Vec<(Region, u16)> },
    /// A scalar-sourced param: its own one-channel region, refilled when its atomic moves.
    Scalar { at: Region, param: usize },
    /// A computed param: the worker's last `[C, BLOCK]` result, copied in as it lands and held.
    Block { at: Region, channels: u16, param: usize },
    /// An Array input: filled from the node's inbox each block, one sample per sample entered.
    Inbox { at: Region, channels: u16, inbox: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stage {
    pub idx: usize,
    /// The occupant of `idx` this was compiled for; a later one waits for its own plan.
    pub serial: u64,
    pub ins: Vec<Source>,
    pub params: Vec<Source>,
    pub outs: Vec<(Region, u16)>,
    /// How many of `params` become ports; the rest reach the node as scalars only.
    pub audio_params: usize,
    /// This stage's one scalar per param: `params.len()` floats, not a block each.
    pub scalars_at: Region,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub stages: Vec<Stage>,
    pub arena_len: usize,
    /// What the device hears: every agreeing `AudioOut`'s input times its gain, summed here.
    pub output: (Region, u16),
    /// Per `AudioOut`: input, gain, and the device channels it lands on; `None` is the whole device.
    pub sinks: Vec<(Source, Source, Option<Vec<u16>>)>,
}

impl Default for Plan {
    fn default() -> Plan {
        Plan { stages: Vec::new(), arena_len: BLOCK, output: (SILENCE, 1), sinks: Vec::new() }
    }
}

impl Plan {
    pub fn reads_inbox(&self, idx: usize, inbox: usize) -> bool {
        self.stages
            .iter()
            .any(|s| s.idx == idx && s.ins.iter().any(|i| matches!(i, Source::Inbox { inbox: n, .. } if *n == inbox)))
    }
}

/// Whether a feed param of the node names, for `slot`, a variable the patch holds.
fn fed(view: &GraphView<'_>, nv: &NodeView<'_>, slot: &str) -> bool {
    goofi_node::feed_decls(nv.manifest)
        .any(|(fed, group, d)| fed == slot && goofi_node::feed_name(nv.params, group, d).is_some_and(|n| view.variables.contains(&n)))
}

/// The inbox an Array input reads — its index among the node's Array inputs — and `None` for an
/// audio one.
pub(crate) fn inbox_of(manifest: &NodeManifest, input: usize) -> Option<usize> {
    let array = |s: &goofi_node::SlotDecl| s.kind != SlotType::Audio;
    array(&manifest.inputs[input]).then(|| manifest.inputs[..input].iter().filter(|s| array(s)).count())
}

fn alloc(channels: u16, len: &mut usize) -> Region {
    let at = *len;
    *len += channels as usize * BLOCK;
    at
}

/// A strip of plain floats, for what is one value per block rather than one per frame.
fn alloc_strip(floats: usize, len: &mut usize) -> Region {
    let at = *len;
    *len += floats;
    at
}

/// One jack's source: silence, its one producer's region, or a sum — which one part also takes
/// when it is this node's own output, so a self-loop never reads and writes one region at once.
fn source_of(parts: Vec<(Region, u16)>, own: &[Region], len: &mut usize) -> Source {
    match parts.as_slice() {
        [] => Source::Silence,
        [(at, channels)] if !own.contains(at) => Source::Region { at: *at, channels: *channels },
        _ => {
            let channels = parts.iter().map(|p| p.1).max().unwrap_or(1);
            Source::Sum { at: alloc(channels, len), channels, parts }
        }
    }
}

/// Whether a binding is a plan edge: a bare reference to a live audio output — a same-engine
/// stream, so the control half never sees it.
pub(crate) fn is_edge<V>(b: &BindingView<'_>, live: &HashMap<Uid, V>) -> bool {
    b.live && b.id.is_none() && b.vars.len() == 1 && b.vars[0].wire().is_some_and(|(p, _)| live.contains_key(&p))
}

/// Kahn over port edges and same-engine references; a `feedback()` node reads the previous block.
/// A `silent` `AudioOut` does not sum, and what a loop or a `disabled` node fed reads silence.
pub fn compile(
    view: &GraphView<'_>,
    all: &HashMap<Uid, Instance>,
    silent: &[Uid],
    disabled: &HashMap<Uid, String>,
) -> (Plan, Vec<(Uid, String)>) {
    let live: HashMap<Uid, &Instance> = all.iter().filter(|(u, _)| !disabled.contains_key(u)).map(|(u, i)| (*u, i)).collect();
    let live = &live;
    let mut wires: HashMap<(Uid, &str), Vec<(Uid, &'static str)>> = HashMap::new();
    for e in view.edges {
        if live.contains_key(&e.consumer.0) && live.contains_key(&e.producer.0) {
            wires.entry(e.consumer).or_default().push(e.producer);
        }
    }
    // An edge is decided over EVERY instance, as the control half decides it: a reference onto
    // a disabled producer stays an edge, one with no region behind it.
    let mut refs: HashMap<(Uid, usize), (Uid, &'static str)> = HashMap::new();
    // A computed param reads the worker's block, a region of its own.
    let mut computed: HashSet<(Uid, usize)> = HashSet::new();
    for (uid, inst) in live {
        let Some(nv) = view.nodes.get(uid) else { continue };
        for (i, d) in inst.manifest.params.iter().enumerate() {
            let bound = nv.bindings.iter().find(|b| b.key.group == d.group && b.key.name == d.name);
            if let Some(b) = bound.filter(|b| is_edge(b, all)) {
                refs.insert((*uid, i), b.vars[0].wire().expect("an edge"));
            } else if bound.is_some_and(|b| b.live && b.id.is_some()) {
                computed.insert((*uid, i));
            }
        }
    }
    let block_width = |uid: Uid, i: usize| computed.contains(&(uid, i)).then(|| live[&uid].param_chans[i].load(Ordering::Relaxed).clamp(1, CEILING));
    let feeds = |consumer: Uid| -> Vec<Uid> {
        let inst = &live[&consumer];
        let mut from: Vec<Uid> = inst
            .manifest
            .inputs
            .iter()
            .filter(|s| s.kind == SlotType::Audio)
            .flat_map(|s| wires.get(&(consumer, s.name)).into_iter().flatten().map(|p| p.0))
            .collect();
        from.extend(
            (0..inst.manifest.params.len())
                .filter_map(|i| refs.get(&(consumer, i)).map(|p| p.0))
                .filter(|p| live.contains_key(p)),
        );
        from
    };
    let inbound: HashMap<Uid, Vec<Uid>> = live
        .iter()
        .map(|(uid, inst)| (*uid, if inst.twin.feedback() { Vec::new() } else { feeds(*uid) }))
        .collect();
    let (order, members) = goofi_runtime::schedule(&inbound, &HashSet::new(), |u| live[&u].twin.feedback());
    let mut faults: Vec<(Uid, String)> =
        members.iter().map(|u| (*u, "in a loop with no feedback node, so it does not run".to_string())).collect();

    let mut plan = Plan::default();
    let parts_of = |uid: Uid, slot: &str, outs_of: &HashMap<(Uid, &'static str), (Region, u16)>| -> Vec<(Region, u16)> {
        wires.get(&(uid, slot)).into_iter().flatten().filter_map(|p| outs_of.get(p).copied()).collect()
    };
    // Pass one lays out every output, so a feedback node's loop in-edge finds its producer's
    // region in pass two; a producer not yet laid out counts as one channel.
    let mut outs_of: HashMap<(Uid, &'static str), (Region, u16)> = HashMap::new();
    for uid in &order {
        let inst = &live[uid];
        let nv = &view.nodes[uid];
        let mut counts: Vec<u16> = inst
            .manifest
            .inputs
            .iter()
            .enumerate()
            .map(|(i, s)| match inbox_of(inst.manifest, i) {
                Some(inbox) => inst.chans[inbox].load(Ordering::Relaxed),
                None => parts_of(*uid, s.name, &outs_of).iter().map(|p| p.1).max().unwrap_or(1),
            })
            .collect();
        counts.extend((0..inst.manifest.params.len()).filter_map(|i| {
            refs.get(&(*uid, i)).map(|p| outs_of.get(p).map_or(1, |o| o.1)).or_else(|| block_width(*uid, i))
        }));
        let scalars: Vec<f64> = inst.manifest.params.iter().map(|d| scalar_of(nv.params, d)).collect();
        let wanted = inst.twin.channels(&counts, &scalars, inst.manifest.outputs.len());
        for (i, o) in inst.manifest.outputs.iter().enumerate() {
            let channels = wanted.get(i).copied().unwrap_or(1).clamp(1, CEILING);
            outs_of.insert((*uid, o.name), (alloc(channels, &mut plan.arena_len), channels));
        }
    }
    for uid in &order {
        let inst = &live[uid];
        let nv = &view.nodes[uid];
        let outs: Vec<(Region, u16)> = inst.manifest.outputs.iter().map(|o| outs_of[&(*uid, o.name)]).collect();
        let own: Vec<Region> = outs.iter().map(|o| o.0).collect();
        let ins: Vec<Source> = inst
            .manifest
            .inputs
            .iter()
            .enumerate()
            .map(|(i, s)| match inbox_of(inst.manifest, i) {
                // Fed by a cable, or by the variable a feed param names; else silence.
                Some(_) if view.wires_into(*uid, s.name).next().is_none() && !fed(view, nv, s.name) => Source::Silence,
                Some(inbox) => {
                    let channels = inst.chans[inbox].load(Ordering::Relaxed);
                    Source::Inbox { at: alloc(channels, &mut plan.arena_len), channels, inbox }
                }
                None => source_of(parts_of(*uid, s.name, &outs_of), &own, &mut plan.arena_len),
            })
            .collect();
        let params: Vec<Source> = (0..inst.manifest.params.len())
            .map(|i| match refs.get(&(*uid, i)).map(|p| outs_of.get(p)) {
                Some(Some(part)) => source_of(vec![*part], &own, &mut plan.arena_len),
                Some(None) => Source::Silence,
                None => match block_width(*uid, i) {
                    Some(channels) => Source::Block { at: alloc(channels, &mut plan.arena_len), channels, param: i },
                    None => Source::Scalar { at: alloc(1, &mut plan.arena_len), param: i },
                },
            })
            .collect();
        if inst.manifest.type_name == crate::nodes::audio_out::TYPE && !silent.contains(uid) {
            // A selection that does not parse plays NOTHING, never the whole device, and the
            // fault carries the parser's message.
            let spec = str_of(view.nodes[uid].params, &inst.manifest.params[crate::nodes::audio_out::P::CHANNELS]);
            match crate::chanmap::parse(&spec) {
                Ok(sel) => plan.sinks.push((
                    ins[0].clone(),
                    params[crate::nodes::audio_out::P::GAIN].clone(),
                    sel,
                )),
                Err(why) => faults.push((*uid, why)),
            }
        }
        let audio_params = inst.twin.audio_params(params.len()).min(params.len());
        let scalars_at = alloc_strip(params.len(), &mut plan.arena_len);
        plan.stages.push(Stage { idx: inst.idx, serial: inst.serial, ins, params, outs, audio_params, scalars_at });
    }
    // The device opens as wide as the furthest channel any sink reaches: its own width, or one
    // past the highest channel its selection names.
    let width = plan
        .sinks
        .iter()
        .map(|(input, _, sel)| {
            let own = match input {
                Source::Region { channels, .. } | Source::Sum { channels, .. } | Source::Inbox { channels, .. } | Source::Block { channels, .. } => {
                    *channels
                }
                Source::Silence | Source::Scalar { .. } => 1,
            };
            sel.as_deref().map_or(own, crate::chanmap::needed_width)
        })
        .max()
        .unwrap_or(1)
        .min(CEILING);
    plan.output = (alloc(width, &mut plan.arena_len), width);
    (plan, faults)
}

/// A `Str` param's text at plan time, off the node's record rather than a control half's consts:
/// the plan is compiled from the desired state, and that is where a channel selection lives.
pub(crate) fn str_of(params: &goofi_node::ParamGroups, d: &goofi_audio_sdk::ParamDecl) -> String {
    match param_of(params, d) {
        goofi_core::Param::Str { value, .. } => value,
        _ => String::new(),
    }
}
