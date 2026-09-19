//! The shared, payload-free ViewSpec algebra: a viewer publishes what it can draw and what it
//! wants reduced, and N specs merge into ONE plan per frame.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

/// The per-axis reduction kernel a viewer asks for on a drawable axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReduceMethod {
    Envelope,
    Subsample,
    Area,
}

/// Which kernels the admitted viewers asked for on ONE axis. A set, not a running pairwise
/// merge, so the fold cannot depend on the order the specs arrive in.
#[derive(Clone, Copy, Default)]
struct MethodSet {
    envelope: bool,
    subsample: bool,
    area: bool,
}

impl MethodSet {
    fn add(&mut self, m: ReduceMethod) {
        match m {
            ReduceMethod::Envelope => self.envelope = true,
            ReduceMethod::Subsample => self.subsample = true,
            ReduceMethod::Area => self.area = true,
        }
    }

    /// The ONE method that must serve every subscriber; a cross-family conflict degrades to
    /// Subsample, the only reduction both an image and a line viewer can draw.
    fn resolve(self) -> ReduceMethod {
        if self.area {
            if self.envelope || self.subsample {
                ReduceMethod::Subsample
            } else {
                ReduceMethod::Area
            }
        } else if self.envelope {
            ReduceMethod::Envelope
        } else {
            ReduceMethod::Subsample
        }
    }
}

/// Desired reduction of one axis to `max` BINS via `method`; `Envelope` emits a (min, max) pair
/// per bin, so it returns `2 * max` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AxisReduce {
    pub dim: i32,
    pub max: usize,
    pub method: ReduceMethod,
}

/// The bins an axis is capped to for a viewer that has declared nothing; Subsample because it is
/// the one kernel every viewer family can draw.
pub const UNDECLARED_MAX: usize = 512;

/// The most elements an undeclared preview carries over ALL its axes — a 512² image — so a frame
/// with many axes cannot cost the stream's whole rate through the two the cap misses.
pub const UNDECLARED_BUDGET: usize = UNDECLARED_MAX * UNDECLARED_MAX;

/// What a producer is asked to fit its readback into for a reader that declared nothing: ONE
/// texel, the cheapest frame that is still a frame. No pixels were asked for, so the metadata is
/// the whole product.
pub const UNDECLARED_BOX: (u32, u32) = (1, 1);

/// The sample depth a viewer can draw: the wire's f32, or 8-bit texels, which cost a quarter of
/// the bytes and are all an image viewer can show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Depth {
    #[default]
    F32,
    U8,
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
    /// The depth this viewer can draw. One stream serves every viewer, so 8-bit is sent only
    /// where every admitted one accepts it.
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
    pub method: ReduceMethod,
}

/// The merged reduction plan for ONE frame. Empty axes ⇒ passthrough (no reduction).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MergedViewSpec {
    pub axes: Vec<PlannedAxis>,
    /// The depth every admitted viewer can draw.
    pub depth: Depth,
}

/// Merge N viewers' specs into ONE concrete plan for THIS frame: specs that do not admit the
/// frame drop out, and each canonical dim folds to `max(max)` plus the union of the kernels.
pub fn plan<R: Reducible + ?Sized>(specs: &[ViewSpec], frame: &R) -> MergedViewSpec {
    // Nothing here can draw this frame, so nothing here has asked for it: the undeclared preview
    // stands in, exactly as it does for a reader that declared nothing at all.
    let (mut axes, depth) =
        fold_axes(specs, frame).unwrap_or_else(|| (undeclared_axes(frame.shape()), Depth::F32));
    aspect_preserve_area(&mut axes, frame.shape());
    MergedViewSpec { axes, depth }
}

/// The preview for a reader that declared nothing, never the full frame: every axis capped at
/// [`UNDECLARED_MAX`], then the largest halved until the frame fits [`UNDECLARED_BUDGET`].
pub fn undeclared_axes(shape: &[usize]) -> Vec<PlannedAxis> {
    let mut caps: Vec<usize> = shape.iter().map(|&n| n.clamp(1, UNDECLARED_MAX)).collect();
    while caps.iter().product::<usize>() > UNDECLARED_BUDGET {
        let Some((largest, _)) = caps.iter().enumerate().max_by_key(|(_, &c)| c) else { break };
        caps[largest] = (caps[largest] / 2).max(1);
    }
    shape
        .iter()
        .zip(caps)
        .enumerate()
        .filter(|(_, (&n, cap))| *cap < n)
        .map(|(dim, (_, max))| PlannedAxis { dim, max, method: ReduceMethod::Subsample })
        .collect()
}

/// Every admitted viewer's asks, folded per dim: `max(max)` and the union of the kernels. What a
/// frame is then actually reduced to is [`plan`]'s business — this is the ask alone. `None` where
/// NOTHING admits the frame: no viewer here can draw it, so none of them has asked for anything.
fn fold_axes<R: Reducible + ?Sized>(specs: &[ViewSpec], frame: &R) -> Option<(Vec<PlannedAxis>, Depth)> {
    let ndim = frame.ndim();
    let mut order: Vec<usize> = Vec::new(); // first-seen dim order → stable output
    let mut folded: HashMap<usize, (usize, MethodSet)> = HashMap::new();
    let mut admitted = 0usize;
    let mut every_u8 = true;
    for spec in specs {
        if !spec.admits(frame) {
            continue;
        }
        admitted += 1;
        every_u8 &= spec.depth == Depth::U8;
        for r in &spec.reduce {
            let Some(d) = canon_dim(r.dim, ndim) else {
                continue;
            };
            let entry = folded.entry(d).or_insert_with(|| {
                order.push(d);
                (0, MethodSet::default())
            });
            entry.0 = entry.0.max(r.max);
            entry.1.add(r.method);
        }
    }
    let axes: Vec<PlannedAxis> = order
        .iter()
        .map(|&d| {
            let (mx, set) = folded[&d];
            PlannedAxis { dim: d, max: mx, method: set.resolve() }
        })
        .collect();
    (admitted > 0).then_some((axes, if every_u8 { Depth::U8 } else { Depth::F32 }))
}

/// What a slot's readers want of its frames: the box to fit the readback into, and the sample
/// width they draw. A producer that can make exactly this spends nothing downstream — no
/// reduction, and no quantization either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewWant {
    pub size: (u32, u32),
    pub depth: Depth,
}

/// The box every admitted viewer would reduce both image axes to with an area kernel — the size
/// a PRODUCER could render instead, making the reduction downstream free — and the depth every
/// one of them draws. This is what the viewers ASKED for, not the fit for one frame, so it does
/// not move when the producer answers it. `None` unless both axes resolve to an area kernel: a
/// plan that subsamples means something else, and a producer must not answer it with an average.
pub fn image_box<R: Reducible + ?Sized>(specs: &[ViewSpec], frame: &R) -> Option<ViewWant> {
    // Nothing admits it, so nobody here is drawing it — and a frame nobody draws needs no pixels.
    let Some((axes, depth)) = fold_axes(specs, frame) else {
        return Some(ViewWant { size: UNDECLARED_BOX, depth: Depth::F32 });
    };
    let axis = |d: usize| axes.iter().find(|a| a.dim == d).filter(|a| a.method == ReduceMethod::Area);
    let (h, w) = (axis(0)?, axis(1)?);
    Some(ViewWant { size: (w.max.clamp(1, u32::MAX as usize) as u32, h.max.clamp(1, u32::MAX as usize) as u32), depth })
}

/// `src` scaled into `box_` with its aspect kept, never enlarged — the one place that rule is
/// stated, so a producer answering [`image_box`] lands exactly where the reduction expected.
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

/// Scale every `Area` axis by one shared factor, so a non-square image keeps its aspect ratio.
fn aspect_preserve_area(axes: &mut [PlannedAxis], shape: &[usize]) {
    let area: Vec<usize> = (0..axes.len())
        .filter(|&i| axes[i].method == ReduceMethod::Area && axes[i].dim < shape.len())
        .collect();
    if area.len() < 2 {
        return;
    }
    let factor = shrink_factor(area.iter().map(|&i| (axes[i].max, shape[axes[i].dim])));
    if factor >= 1.0 {
        return;
    }
    for &i in &area {
        axes[i].max = ((shape[axes[i].dim] as f64 * factor).round() as usize).max(1);
    }
}
