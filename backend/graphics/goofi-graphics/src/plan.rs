//! The plan: what the render thread runs each tick, compiled at settle from the settled view —
//! an order, each stage's inputs and size, and the cells the tick reads demand from and writes
//! its readback into.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use goofi_core::SlotType;
use goofi_node::{GraphView, ParamDecl, Uid};

use crate::gpu::Want;
use crate::half::Cells;
use crate::scan::Built;
use crate::Instance;

/// What a slot's readers asked for, in the ONE cell [`Stage::wants`] reads: the box on two 16-bit
/// axes — [`MAX_SIZE`] is 8192, so they fit — and the depth on one bit above them. Zero is the
/// frame itself at f32, which is what a snapshot and a variable following the slot need. One packer
/// and one reader, so the cell has one owner and no second field to keep in step.
pub fn pack(want: Option<goofi_view::ViewWant>) -> u64 {
    want.map_or(0, |w| {
        let depth = u64::from(w.depth == goofi_view::Depth::U8) << 32;
        depth | (u64::from(w.size.0.min(MAX_SIZE)) << 16) | u64::from(w.size.1.min(MAX_SIZE))
    })
}

/// The cell read back, or `None` where nothing narrowed it.
fn unpack(cell: u64) -> Option<goofi_view::ViewWant> {
    (cell != 0).then_some(goofi_view::ViewWant {
        size: (((cell >> 16) & 0xffff) as u32, (cell & 0xffff) as u32),
        depth: if cell & (1 << 32) == 0 { goofi_view::Depth::F32 } else { goofi_view::Depth::U8 },
    })
}

/// What a chain that can follow nothing falls back to: the patch's two default-size variables.
pub fn generator(view: &GraphView<'_>) -> (u32, u32) {
    let axis = |name: &str| match view.variables.get(name) {
        Some(goofi_core::variables::VariableValue::Int(v)) => (*v).clamp(1, MAX_SIZE as i64) as u32,
        _ => goofi_core::variables::DEFAULT_SIZE,
    };
    (axis("system.default_width"), axis("system.default_height"))
}
/// The widest a node may ask for on either axis.
pub const MAX_SIZE: u32 = goofi_core::texture::MAX_SIZE;

/// The size held in a node's param atomics, whose universal `common` group starts at `base`.
pub fn asked(params: &[AtomicU64], base: usize) -> (u32, u32) {
    let axis = |i: usize| {
        let held = params.get(base + i).map_or(0.0, |a| f64::from_bits(a.load(Ordering::Relaxed)));
        held.clamp(0.0, MAX_SIZE as f64) as u32
    };
    (axis(0), axis(1))
}

pub enum Input {
    /// Another stage's output, by index into `Plan::stages`.
    Stage(usize),
    /// The k-th ARRAY input's upload cell.
    Upload(usize),
    /// Nothing is wired: the shared transparent texture.
    None,
}

/// A stage's armed recording, from SETTLED state: the names the recorder files it under. Whether
/// a recording actually runs is the recorder's own, and is asked of it once per tick.
pub struct Record {
    pub node: String,
    pub slot: String,
    pub quality: goofi_core::record::VideoQuality,
}

pub enum Pass {
    Shader(Built),
    Host { source: crate::producer::Source, program: Option<(Arc<str>, Built)> },
}

pub struct Stage {
    pub uid: Uid,
    pub pass: Pass,
    pub inputs: Vec<Input>,
    /// How many state buffers this stage carries between two ticks.
    pub state: usize,
    pub size: (u32, u32),
    pub natural: (bool, bool),
    pub decls: &'static [ParamDecl],
    pub cells: Arc<Cells>,
    /// The window on the machine's own screen this stage draws into, once one is open.
    pub window: Option<goofi_window::Id>,
    /// This output slot's arming, as the last settle left it.
    pub record: Option<Record>,
}

impl Stage {
    /// What size each reader of this stage's output wants it at, or nothing where that reader is
    /// absent: a window on the machine's own screen — the one reader the transport cannot count —
    /// and a subscriber on its data service. A screen is always the frame's own size.
    pub fn wants(&self, recording: bool) -> [Option<(u32, u32)>; 4] {
        let mut wants = [None; 4];
        wants[Want::Screen as usize] = self.window.map(|_| self.size);
        let asked = unpack(self.cells.tap_box.load(Ordering::Relaxed));
        let tap = asked.map_or(self.size, |w| goofi_view::fit(self.size, w.size));
        // The two taps are one reader in two formats, so at most one of them is ever wanted.
        let which = match asked.map(|w| w.depth) {
            Some(goofi_view::Depth::U8) => Want::TapU8,
            _ => Want::Tap,
        };
        wants[which as usize] = self.cells.readers.load(Ordering::Relaxed).then_some(tap);
        // A recorder takes the frame at its own size: a recording is the evidence, and evidence
        // is never fitted to a box somebody's screen happened to have.
        wants[Want::Record as usize] = (recording && self.record.is_some()).then_some(self.size);
        wants
    }

    /// Whether anything reads this stage's output at all.
    pub fn read(&self, recording: bool) -> bool {
        self.wants(recording).iter().any(|w| w.is_some())
    }
}

#[derive(Default)]
pub struct Plan {
    pub stages: Vec<Stage>,
}

impl Plan {
    /// Which stages render this tick: every stage a reader reaches backwards over the edges. A
    /// node nobody reads costs nothing, which is what makes a big idle patch free.
    pub fn demanded(&self, recording: bool) -> Vec<bool> {
        let mut want = vec![false; self.stages.len()];
        let mut stack: Vec<usize> =
            (0..self.stages.len()).filter(|&i| self.stages[i].read(recording)).collect();
        while let Some(i) = stack.pop() {
            if std::mem::replace(&mut want[i], true) {
                continue;
            }
            for input in &self.stages[i].inputs {
                if let Input::Stage(j) = input {
                    stack.push(*j);
                }
            }
        }
        want
    }
}

/// The order, the sizes and the wiring, from settled state. A node whose class answers `feedback`
/// ignores its in-edges and runs first, reading its producer's texture as the last tick left it;
/// a loop with no such node is excluded and named.
pub fn compile(
    view: &GraphView<'_>,
    live: &HashMap<Uid, Instance>,
    windows: &HashMap<Uid, goofi_window::Id>,
) -> (Plan, Vec<(Uid, String)>) {
    let wires = texture_wires(view, live);
    let feeds = |consumer: Uid| -> Vec<Uid> {
        let inst = &live[&consumer];
        inst.class
            .manifest
            .inputs
            .iter()
            .filter(|s| s.kind == SlotType::Texture)
            .filter_map(|s| wires.get(&(consumer, s.name)).copied())
            .collect()
    };
    // A pass cannot read the texture it writes, so a node wired to ITSELF is out even when it is
    // the feedback node — a loop needs a second stage for the frame to age in.
    let mine: HashSet<Uid> = live.keys().copied().filter(|u| feeds(*u).contains(u)).collect();
    let inbound: HashMap<Uid, Vec<Uid>> = live
        .iter()
        .map(|(uid, i)| (*uid, if i.class.feedback && !mine.contains(uid) { Vec::new() } else { feeds(*uid) }))
        .collect();
    let (order, members) = goofi_runtime::schedule(&inbound, &mine, |u| live[&u].class.feedback);
    let mut faults: Vec<(Uid, String)> = members
        .iter()
        .map(|u| (*u, "in a loop with no feedback node, so it does not render".to_string()))
        .collect();
    faults.extend(mine.iter().map(|u| (*u, "wired to its own output, so it does not render".to_string())));

    let sizes = sizes(view, live);
    let at: HashMap<Uid, usize> = order.iter().enumerate().map(|(i, u)| (*u, i)).collect();
    let mut stages = Vec::with_capacity(order.len());
    for uid in &order {
        let inst = &live[uid];
        if let crate::scan::Kind::Shader(built) = &inst.class.kind {
            if let Some(Err(why)) = built.get() { faults.push((*uid, format!("shader: {why}"))); }
        }
        if let Some((_, built)) = &inst.program {
            if let Some(Err(why)) = built.get() { faults.push((*uid, format!("texture program: {why}"))); }
        }
        let manifest = inst.class.manifest;
        let inputs = manifest
            .inputs
            .iter()
            .map(|s| match s.kind {
                SlotType::Texture => wires.get(&(*uid, s.name)).and_then(|p| at.get(p)).map_or(Input::None, |i| Input::Stage(*i)),
                // From the settled view: an unwired ARRAY input is transparent black.
                _ if view.wires_into(*uid, s.name).next().is_none() => Input::None,
                _ => Input::Upload(crate::shader::array_inputs(manifest).position(|n| n == s.name).expect("an array input")),
            })
            .collect();
        stages.push(Stage {
            uid: *uid,
            pass: match &inst.class.kind {
                crate::scan::Kind::Shader(built) => Pass::Shader(built.clone()),
                crate::scan::Kind::Host(_) => Pass::Host { source: inst.source.clone().expect("host source"), program: inst.program.clone() },
            },
            inputs,
            state: inst.class.state.len(),
            size: sizes[uid],
            natural: (inst.asked().0 == 0, inst.asked().1 == 0),
            decls: inst.class.manifest.params,
            cells: inst.cells.clone(),
            window: windows.get(uid).copied(),
            record: recorded(view, *uid, inst),
        });
    }
    (Plan { stages }, faults)
}

/// The armed TEXTURE output of a node, from the settled view. A slot the node does not have, or
/// one that carries no texture, is nothing to record.
fn recorded(view: &GraphView<'_>, uid: Uid, inst: &Instance) -> Option<Record> {
    let nv = view.nodes.get(&uid)?;
    let slot = inst
        .class
        .manifest
        .outputs
        .iter()
        .find(|o| o.kind == SlotType::Texture && nv.recorded.iter().any(|r| r.slot == o.name))?;
    Some(Record { node: nv.name.to_string(), slot: slot.name.to_string(),
        quality: nv.recorded.iter().find(|r| r.slot == slot.name)?.quality })
}

/// Every wire between two live nodes, by the input it lands on.
fn texture_wires<'v>(view: &GraphView<'v>, live: &HashMap<Uid, Instance>) -> HashMap<(Uid, &'v str), Uid> {
    let mut wires = HashMap::new();
    for e in view.edges {
        if live.contains_key(&e.consumer.0) && live.contains_key(&e.producer.0) {
            wires.insert(e.consumer, e.producer.0);
        }
    }
    wires
}

/// Every live node's texture size, from settled state alone — the engine reads it to size a
/// window, and the plan to size a target.
pub fn sizes(view: &GraphView<'_>, live: &HashMap<Uid, Instance>) -> HashMap<Uid, (u32, u32)> {
    let wires = texture_wires(view, live);
    let fallback = generator(view);
    let mut sizes = HashMap::new();
    for uid in live.keys() {
        size_of(*uid, live, &wires, fallback, &mut sizes, &mut Vec::new());
    }
    sizes
}

/// A node's size: what `common/width` and `common/height` hold, and for a zero on an axis the
/// first wired texture input's size on that axis — or, with no texture behind it, the frame its
/// host made or its first array input uploaded. A chain that follows itself, or one that follows
/// nothing, takes the patch's default.
fn size_of(
    uid: Uid,
    live: &HashMap<Uid, Instance>,
    wires: &HashMap<(Uid, &str), Uid>,
    fallback: (u32, u32),
    sizes: &mut HashMap<Uid, (u32, u32)>,
    visiting: &mut Vec<Uid>,
) -> (u32, u32) {
    if let Some(known) = sizes.get(&uid) {
        return *known;
    }
    let (w, h) = live.get(&uid).map_or((0, 0), Instance::asked);
    let mut answer = (w, h);
    // A chain that follows ITSELF cannot answer; what it asked for on either axis still stands.
    if visiting.contains(&uid) {
        return (if w == 0 { fallback.0 } else { w }, if h == 0 { fallback.1 } else { h });
    }
    visiting.push(uid);
    if w == 0 || h == 0 {
        let behind = live.get(&uid).and_then(|inst| {
            inst.class
                .manifest
                .inputs
                .iter()
                .filter(|s| s.kind == SlotType::Texture)
                .find_map(|s| wires.get(&(uid, s.name)).copied())
        });
        let (fw, fh) = match behind {
            Some(p) => size_of(p, live, wires, fallback, sizes, visiting),
            None => live.get(&uid).and_then(|i| {
                i.source.as_ref().and_then(|s| s.lock().as_ref().map(|f| f.size))
                    .or_else(|| crate::half::unpack(i.cells.uploaded.load(Ordering::Relaxed)))
            }).unwrap_or(fallback),
        };
        answer = (if w == 0 { fw } else { w }, if h == 0 { fh } else { h });
    }
    visiting.pop();
    sizes.insert(uid, answer);
    answer
}
