//! The shared, payload-free ViewSpec algebra: a viewer publishes what it can draw and what it
//! wants reduced, and N specs merge into ONE plan per frame.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// The Data kind a viewer draws; the tags are goofi-core's wire dtype tags, restated to keep
/// this crate free of that dependency.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewDtype {
    Array,
    String,
    Table,
}

impl ViewDtype {
    pub fn tag(self) -> u8 {
        match self {
            ViewDtype::Array => 0,
            ViewDtype::String => 1,
            ViewDtype::Table => 2,
        }
    }
}

/// Comparison operators for the dim-count and per-dim length constraints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DimCmp {
    Lt,
    Le,
    Eq,
    Ge,
    Gt,
}

impl DimCmp {
    /// `actual <op> n`.
    pub fn holds(self, actual: usize, n: usize) -> bool {
        match self {
            DimCmp::Lt => actual < n,
            DimCmp::Le => actual <= n,
            DimCmp::Eq => actual == n,
            DimCmp::Ge => actual >= n,
            DimCmp::Gt => actual > n,
        }
    }
}

/// A constraint on ONE dimension's length; a negative `dim` counts from the end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DimConstraint {
    pub dim: i32,
    pub cmp: DimCmp,
    pub n: usize,
}

/// What a viewer wants of one axis: at most `max` entries, subsampled, or the axis whole. A dim
/// it names neither way it has no opinion on; the fold reads nothing into the silence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AxisReduce {
    pub dim: i32,
    pub max: Ask,
}

/// One axis's ask, on the wire a count or the word `whole`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    Cap(usize),
    Whole,
}

impl Serialize for Ask {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Ask::Cap(n) => s.serialize_u64(*n as u64),
            Ask::Whole => s.serialize_str("whole"),
        }
    }
}

impl<'de> Deserialize<'de> for Ask {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Cap(usize),
            Word(String),
        }
        match Raw::deserialize(d)? {
            Raw::Cap(n) => Ok(Ask::Cap(n)),
            Raw::Word(w) if w == "whole" => Ok(Ask::Whole),
            Raw::Word(w) => Err(serde::de::Error::custom(format!("an axis ask is a count or `whole`, not `{w}`"))),
        }
    }
}

/// The entries an axis is capped to for a viewer that has declared nothing.
pub const UNDECLARED_MAX: usize = 512;

/// The most elements an undeclared preview carries over ALL its axes — 512² — so a frame with
/// many axes cannot cost the stream's whole rate through the two the cap misses.
pub const UNDECLARED_BUDGET: usize = UNDECLARED_MAX * UNDECLARED_MAX;

/// What a producer is asked to fit its readback into for a reader that declared nothing: ONE
/// texel, the cheapest frame that is still a frame. No pixels were asked for, so the metadata is
/// the whole product.
pub const UNDECLARED_BOX: (u32, u32) = (1, 1);

/// The sample depth a reader takes, narrowest first: 8-bit texels, a half float, or the f32 the
/// wire itself carries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Depth {
    U8,
    F16,
    #[default]
    F32,
}

/// One viewer's full declaration: what it can draw + what it wants reduced.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewSpec {
    /// The Data kind this viewer draws.
    pub dtype: ViewDtype,
    /// Dim-count comparisons — ALL must hold to admit (empty ⇒ any ndim).
    #[serde(default)]
    pub ndim: Vec<(DimCmp, usize)>,
    /// Per-dim length comparisons (ALL must hold to admit).
    #[serde(default)]
    pub dims: Vec<DimConstraint>,
    /// Desired per-axis reductions.
    #[serde(default)]
    pub reduce: Vec<AxisReduce>,
    /// Its caps are ONE box: the dims it caps shrink by one shared factor, so their ratio holds.
    /// Without it each cap stands alone.
    #[serde(default)]
    pub aspect: bool,
    /// The depth this viewer can draw. One stream serves every viewer, so it is as narrow as
    /// the widest admitted ask.
    #[serde(default)]
    pub depth: Depth,
}

/// A payload frame the reducer can query for shape.
pub trait Reducible {
    /// 0=array, 1=string, 2=table (matches the wire dtype tag).
    fn dtype_tag(&self) -> u8;
    /// Number of dimensions (0 for a non-array payload).
    fn ndim(&self) -> usize;
    /// The shape (empty for a non-array payload).
    fn shape(&self) -> &[usize];
}

/// Map a possibly-negative axis index to `0..ndim`, or `None` if out of range.
pub fn canon_dim(dim: i32, ndim: usize) -> Option<usize> {
    let d = if dim < 0 { ndim.checked_sub(dim.unsigned_abs() as usize)? } else { dim as usize };
    (d < ndim).then_some(d)
}

impl ViewSpec {
    /// Whether this viewer can draw `frame`, and so joins the merge.
    pub fn admits<R: Reducible + ?Sized>(&self, frame: &R) -> bool {
        if frame.dtype_tag() != self.dtype.tag() {
            return false;
        }
        if frame.dtype_tag() != ViewDtype::Array.tag() {
            return true;
        }
        let ndim = frame.ndim();
        for &(cmp, n) in &self.ndim {
            if !cmp.holds(ndim, n) {
                return false;
            }
        }
        for c in &self.dims {
            let Some(d) = canon_dim(c.dim, ndim) else {
                return false;
            };
            if !c.cmp.holds(frame.shape()[d], c.n) {
                return false;
            }
        }
        true
    }
}

/// One planned axis reduction, with `dim` already canonical.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlannedAxis {
    pub dim: usize,
    pub max: usize,
}

/// The merged reduction plan for ONE frame. Empty axes ⇒ passthrough (no reduction).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MergedViewSpec {
    pub axes: Vec<PlannedAxis>,
    /// The depth every admitted viewer can draw.
    pub depth: Depth,
}

/// Merge N viewers' specs into ONE concrete plan for THIS frame: specs that do not admit the
/// frame drop out, and each canonical dim folds to `max(max)`.
pub fn plan<R: Reducible + ?Sized>(specs: &[ViewSpec], frame: &R) -> MergedViewSpec {
    // Nothing here can draw this frame, so nothing here has asked for it: the undeclared preview
    // stands in, exactly as it does for a reader that declared nothing at all.
    let Some(fold) = fold_axes(specs, frame) else {
        return MergedViewSpec { axes: undeclared_axes(frame.shape()), depth: Depth::F32 };
    };
    let mut axes = fold.axes;
    if fold.aspect {
        aspect_preserve(&mut axes, frame.shape());
    }
    MergedViewSpec { axes, depth: fold.depth }
}

/// The preview for a reader that declared nothing, never the full frame: every axis capped at
/// [`UNDECLARED_MAX`], then the largest halved until the frame fits [`UNDECLARED_BUDGET`].
pub fn undeclared_axes(shape: &[usize]) -> Vec<PlannedAxis> {
    let mut caps: Vec<usize> = shape.iter().map(|&n| n.clamp(1, UNDECLARED_MAX)).collect();
    // Checked: eight axes at the cap overflow a plain product, and an overflow reads as "fits".
    let over = |caps: &[usize]| caps.iter().try_fold(1usize, |p, &c| p.checked_mul(c)).is_none_or(|p| p > UNDECLARED_BUDGET);
    while over(&caps) {
        let Some((largest, _)) = caps.iter().enumerate().max_by_key(|(_, &c)| c) else { break };
        caps[largest] = (caps[largest] / 2).max(1);
    }
    shape
        .iter()
        .zip(caps)
        .enumerate()
        .filter(|(_, (&n, cap))| *cap < n)
        .map(|(dim, (_, max))| PlannedAxis { dim, max })
        .collect()
}

/// What the admitted specs asked of a frame, folded: the axes, the depth they all take, and
/// whether the caps are one box (every spec that caps a dim is an `aspect` one).
struct Fold {
    axes: Vec<PlannedAxis>,
    depth: Depth,
    aspect: bool,
}

/// Every admitted viewer's asks, folded per dim to `max(max)`. A dim an admitted viewer asks
/// whole is not reduced for anyone. `None` where NOTHING admits the frame: no viewer here can
/// draw it, so none of them has asked for anything.
fn fold_axes<R: Reducible + ?Sized>(specs: &[ViewSpec], frame: &R) -> Option<Fold> {
    let ndim = frame.ndim();
    let mut order: Vec<usize> = Vec::new(); // first-seen dim order → stable output
    let mut folded: HashMap<usize, usize> = HashMap::new();
    let mut whole: HashSet<usize> = HashSet::new();
    let mut admitted = 0usize;
    let mut depth = Depth::U8;
    let mut aspect = true;
    for spec in specs {
        if !spec.admits(frame) {
            continue;
        }
        admitted += 1;
        depth = depth.max(spec.depth);
        for r in &spec.reduce {
            let Some(d) = canon_dim(r.dim, ndim) else {
                continue;
            };
            let Ask::Cap(max) = r.max else {
                whole.insert(d);
                continue;
            };
            aspect &= spec.aspect;
            let entry = folded.entry(d).or_insert_with(|| {
                order.push(d);
                0
            });
            *entry = (*entry).max(max);
        }
    }
    let axes: Vec<PlannedAxis> =
        order.iter().filter(|d| !whole.contains(d)).map(|&d| PlannedAxis { dim: d, max: folded[&d] }).collect();
    (admitted > 0).then_some(Fold { axes, depth, aspect })
}

/// What a slot's readers want of its frames: the box to fit the readback into, and the sample
/// width they draw. A producer that can make exactly this spends nothing downstream — no
/// reduction, and no quantization either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewWant {
    pub size: (u32, u32),
    pub depth: Depth,
}

/// The box the `aspect` specs asked for on dims 0 and 1 — the size a PRODUCER could render
/// instead, making the reduction downstream free — and the depth every admitted spec takes. This
/// is what was ASKED, not the fit for one frame, so it does not move when the producer answers
/// it. `None` where a spec caps those dims independently: that is no box to render into.
pub fn asked_box<R: Reducible + ?Sized>(specs: &[ViewSpec], frame: &R) -> Option<ViewWant> {
    // Nothing admits it, so nobody here is drawing it — and a frame nobody draws needs no pixels.
    let Some(fold) = fold_axes(specs, frame) else {
        return Some(ViewWant { size: UNDECLARED_BOX, depth: Depth::F32 });
    };
    if !fold.aspect {
        return None;
    }
    let axis = |d: usize| fold.axes.iter().find(|a| a.dim == d);
    let (h, w) = (axis(0)?, axis(1)?);
    Some(ViewWant { size: (w.max.clamp(1, u32::MAX as usize) as u32, h.max.clamp(1, u32::MAX as usize) as u32), depth: fold.depth })
}

/// `src` scaled into `box_` with its aspect kept, never enlarged — the one place that rule is
/// stated, so a producer answering [`asked_box`] lands exactly where the reduction expected.
pub fn fit(src: (u32, u32), box_: (u32, u32)) -> (u32, u32) {
    let factor = shrink_factor([(box_.0 as usize, src.0 as usize), (box_.1 as usize, src.1 as usize)]);
    if factor >= 1.0 {
        return src;
    }
    (((src.0 as f64 * factor).round() as u32).max(1), ((src.1 as f64 * factor).round() as u32).max(1))
}

/// The one shared factor that keeps an aspect ratio: the tightest of `max / len`, never above 1.
fn shrink_factor(pairs: impl IntoIterator<Item = (usize, usize)>) -> f64 {
    let factor = pairs
        .into_iter()
        .map(|(max, len)| max as f64 / len.max(1) as f64)
        .fold(f64::INFINITY, f64::min);
    if factor.is_finite() {
        factor
    } else {
        1.0
    }
}

/// Scale every capped axis by one shared factor, so the ratio between them holds.
fn aspect_preserve(axes: &mut [PlannedAxis], shape: &[usize]) {
    let capped: Vec<usize> = (0..axes.len()).filter(|&i| axes[i].dim < shape.len()).collect();
    if capped.len() < 2 {
        return;
    }
    let factor = shrink_factor(capped.iter().map(|&i| (axes[i].max, shape[axes[i].dim])));
    if factor >= 1.0 {
        return;
    }
    for &i in &capped {
        axes[i].max = ((shape[axes[i].dim] as f64 * factor).round() as usize).max(1);
    }
}
