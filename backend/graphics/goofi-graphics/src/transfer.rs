//! What an ARRAY frame becomes on its way to a texture. The default is the frame's own texels; the
//! other modes DRAW it, the way goofi's viewers draw one — at the node's own size, on transparent
//! ground, so a plot composites like any other texture.

use std::sync::atomic::{AtomicU64, Ordering};

use goofi_core::{Data, SlotType, Value};
use goofi_node::{NodeManifest, ParamDecl, ParamSpec};

use crate::plan::GENERATOR;

/// One arrival as the render thread takes it: `width * height * 4` f16 texels, row 0 the top.
pub struct Upload {
    pub width: u32,
    pub height: u32,
    pub texels: Vec<u16>,
}

/// The colours a series is drawn in, in the viewers' own order (`frontend/src/lib/viewers/palette.ts`).
const SERIES: [[f32; 3]; 8] = [
    [0.478, 0.718, 1.000],
    [0.710, 0.549, 1.000],
    [0.365, 0.816, 0.604],
    [1.000, 0.718, 0.380],
    [1.000, 0.478, 0.635],
    [0.604, 0.639, 0.702],
    [0.773, 0.784, 0.839],
    [0.431, 0.463, 0.525],
];

/// Half the width of a drawn path, in texels.
const STROKE: f32 = 0.8;
/// The dot that says where a trajectory is NOW; a viewer always draws it.
const HEAD: f32 = 2.5;
/// Past this many points a plot draws no per-sample dots — the viewers' own cut-off.
const DOTS_MAX: usize = 800;
/// The most channel pairs a trajectory draws, guarding `n * (n - 1) / 2`.
const MAX_PAIRS: usize = 64;
/// How far a trajectory's held range shrinks each frame before it grows to fit.
const SHRINK: f32 = 0.01;
/// The slack a trajectory leaves around the path it framed.
const MARGIN: f32 = 0.1;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Texture,
    Line,
    Trajectory,
}

const MODES: &[&str] = &["texture", "line", "trajectory"];

/// How many params one ARRAY input carries.
pub const PER_INPUT: usize = 7;

/// The params EVERY ARRAY input carries: what its frame becomes, and how it is drawn. They are the
/// options goofi's own viewers offer, so a plot on a texture and a plot in a panel answer to one
/// vocabulary.
pub fn decls(input: &'static str) -> Vec<ParamDecl> {
    let flag = |name, default, doc| ParamDecl {
        group: input,
        name,
        spec: ParamSpec::Bool { default },
        expression: None,
        doc: Some(doc),
    };
    let float = |name, default, min, max, doc| ParamDecl {
        group: input,
        name,
        spec: ParamSpec::Float { default, min, max },
        expression: None,
        doc: Some(doc),
    };
    vec![
        ParamDecl {
            group: input,
            name: "mode",
            spec: ParamSpec::Str { default: "texture", options: MODES, refresh: false },
            expression: None,
            doc: Some(
                "What the frame becomes: its own texels, a line plot of it, or a trajectory \
                 through its channels.",
            ),
        },
        flag("auto", true, "Take the range from the frame itself."),
        float("min", -1.0, -1.0e9, 1.0e9, "The bottom of the range, where `auto` is off."),
        float("max", 1.0, -1.0e9, 1.0e9, "The top of the range, where `auto` is off."),
        flag("log_x", false, "Space the samples by decade rather than evenly."),
        flag("log_y", false, "Draw the values by decade rather than evenly."),
        float("points", 0.0, 0.0, 12.0, "Radius of a dot per sample; 0 draws the path alone."),
    ]
}

/// Every ARRAY input, in the order their uploads are numbered.
pub fn array_inputs(m: &'static NodeManifest) -> impl Iterator<Item = &'static str> {
    m.inputs.iter().filter(|s| s.kind != SlotType::Texture).map(|s| s.name)
}

/// One ARRAY input's transfer, as its params stand right now.
pub struct Transfer {
    pub mode: Mode,
    auto: bool,
    min: f32,
    max: f32,
    log_x: bool,
    log_y: bool,
    points: f32,
}

fn held(params: &[AtomicU64], base: usize, i: usize) -> u64 {
    params.get(base + i).map_or(0, |a| a.load(Ordering::Relaxed))
}

/// What the transfer at `base` was drawn under, so a tick that changed nothing draws nothing.
pub fn bits(params: &[AtomicU64], base: usize) -> [u64; PER_INPUT] {
    std::array::from_fn(|i| held(params, base, i))
}

pub fn read(params: &[AtomicU64], base: usize) -> Transfer {
    let at = |i: usize| f64::from_bits(held(params, base, i));
    Transfer {
        mode: match at(0).round() as i64 {
            1 => Mode::Line,
            2 => Mode::Trajectory,
            _ => Mode::Texture,
        },
        auto: at(1) != 0.0,
        min: at(2) as f32,
        max: at(3) as f32,
        log_x: at(4) != 0.0,
        log_y: at(5) != 0.0,
        points: at(6) as f32,
    }
}

/// The size a drawing is made at: what the node asked for, and the generator's own where it asked
/// for nothing. A node with an ARRAY input has no texture behind it to follow.
pub fn drawn_size(asked: (u32, u32)) -> (u32, u32) {
    (if asked.0 == 0 { GENERATOR } else { asked.0 }, if asked.1 == 0 { GENERATOR } else { asked.1 })
}

/// The range a trajectory holds between frames: it shrinks a hair each frame and then grows to
/// fit, so a path stays framed without jittering.
#[derive(Default)]
pub struct Range {
    lo: Option<f32>,
    hi: Option<f32>,
}

impl Range {
    fn fit(&mut self, lo: f32, hi: f32) -> (f32, f32) {
        // Shrunk towards the centre BEFORE it grows to fit: without that a range one spike opened
        // never closes again.
        let held = self
            .lo
            .zip(self.hi)
            .map(|(l, h)| (l * (1.0 - SHRINK) + h * SHRINK, h * (1.0 - SHRINK) + l * SHRINK));
        let (l, h) = held.unwrap_or((lo, hi));
        let (l, h) = (l.min(lo), h.max(hi));
        (self.lo, self.hi) = (Some(l), Some(h));
        let margin = (h - l).abs() * MARGIN;
        sane(l - margin, h + margin)
    }
}

/// A range a drawing can map onto: an empty or degenerate one becomes a unit window at its centre.
fn sane(lo: f32, hi: f32) -> (f32, f32) {
    if lo.is_finite() && hi.is_finite() && hi > lo {
        return (lo, hi);
    }
    let c = if lo.is_finite() && hi.is_finite() { (lo + hi) / 2.0 } else { 0.0 };
    (c - 1.0, c + 1.0)
}

/// A frame as rows of samples: `[N]` is one row, `[C, N]` is C. Nothing else plots.
fn rows_of(frame: &Data) -> Option<(usize, usize, Vec<f32>)> {
    let Value::Array(a) = frame.value() else { return None };
    let (rows, n) = match *a.shape() {
        [n] => (1, n),
        [c, n] => (c, n),
        _ => return None,
    };
    (rows > 0 && n > 0).then(|| (rows, n, floats(a.as_bytes())))
}

fn floats(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().expect("four bytes"))).collect()
}

impl Transfer {
    /// The texels for one frame: the frame's own, or a drawing of it at `size`.
    pub fn upload(&self, frame: &Data, size: (u32, u32), range: &mut Range) -> Option<Upload> {
        match self.mode {
            Mode::Texture => texels(frame),
            Mode::Line => self.line(frame, size),
            Mode::Trajectory => self.trajectory(frame, size, range),
        }
    }

    /// Where a value sits on the axis: itself, or its decade. A value log cannot take is a gap.
    fn ordinate(&self, v: f32) -> f32 {
        if !self.log_y {
            return v;
        }
        if v > 0.0 {
            v.log10()
        } else {
            f32::NAN
        }
    }

    /// Where sample `i` of `n` sits across the frame.
    fn abscissa(&self, i: usize, n: usize) -> f32 {
        if n <= 1 {
            return 0.5;
        }
        if self.log_x {
            ((i + 1) as f32).log10() / (n as f32).log10()
        } else {
            i as f32 / (n - 1) as f32
        }
    }

    /// The range a drawing maps onto the frame: the data's own, or the one asked for.
    fn span(&self, values: impl Iterator<Item = f32>) -> (f32, f32) {
        if !self.auto {
            return sane(self.ordinate(self.min), self.ordinate(self.max));
        }
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for v in values.filter(|v| v.is_finite()) {
            lo = lo.min(v);
            hi = hi.max(v);
        }
        sane(lo, hi)
    }

    /// Min/max decimation: past two samples a column, each column folds to its lowest and its
    /// highest, in the order the path walks them — the viewers' own rule.
    fn fold(&self, s: &[f32], cols: usize) -> Vec<(usize, f32)> {
        let n = s.len();
        if n <= cols * 2 {
            return s.iter().copied().enumerate().collect();
        }
        let buckets = cols.max(1);
        let width = n as f32 / buckets as f32;
        let mut out = Vec::with_capacity(buckets * 2);
        for b in 0..buckets {
            let start = ((b as f32 * width) as usize).min(n - 1);
            let end = ((((b + 1) as f32 * width) as usize).max(start + 1)).min(n);
            let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
            for v in s[start..end].iter().filter(|v| v.is_finite()) {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
            let (lo, hi) = if lo.is_finite() { (lo, hi) } else { (f32::NAN, f32::NAN) };
            out.push((start, lo));
            out.push((end - 1, hi));
        }
        out
    }

    fn line(&self, frame: &Data, size: (u32, u32)) -> Option<Upload> {
        let mut canvas = Canvas::new(size);
        let Some((rows, n, flat)) = rows_of(frame) else { return Some(canvas.upload()) };
        let series: Vec<Vec<f32>> = (0..rows)
            .map(|c| flat[c * n..(c + 1) * n].iter().map(|v| self.ordinate(*v)).collect())
            .collect();
        let (lo, hi) = self.span(series.iter().flatten().copied());
        for (k, s) in series.iter().enumerate() {
            let colour = SERIES[k % SERIES.len()];
            let pts: Vec<(f32, f32)> = if n == 1 {
                // One sample has no width to run across, so it draws as the level it is.
                let y = canvas.y(s[0], lo, hi);
                vec![(0.0, y), (canvas.w as f32, y)]
            } else {
                self.fold(s, canvas.w)
                    .into_iter()
                    .map(|(i, v)| (canvas.x(self.abscissa(i, n)), canvas.y(v, lo, hi)))
                    .collect()
            };
            canvas.path(&pts, STROKE, colour);
            if self.points > 0.0 && pts.len() <= DOTS_MAX {
                for p in &pts {
                    canvas.stamp(p.0, p.1, self.points, colour);
                }
            }
        }
        Some(canvas.upload())
    }

    fn trajectory(&self, frame: &Data, size: (u32, u32), range: &mut Range) -> Option<Upload> {
        let mut canvas = Canvas::new(size);
        let Some((rows, n, flat)) = rows_of(frame) else { return Some(canvas.upload()) };
        if rows < 2 {
            return Some(canvas.upload());
        }
        let (lo, hi) = if self.auto {
            let finite = flat.iter().copied().filter(|v| v.is_finite());
            let (mut l, mut h) = (f32::INFINITY, f32::NEG_INFINITY);
            for v in finite {
                l = l.min(v);
                h = h.max(v);
            }
            if l.is_finite() {
                range.fit(l, h)
            } else {
                sane(f32::NAN, f32::NAN)
            }
        } else {
            sane(self.min, self.max)
        };
        for (k, (i, j)) in pairs(rows).into_iter().enumerate() {
            let colour = SERIES[k % SERIES.len()];
            let pts: Vec<(f32, f32)> = (0..n)
                .map(|t| {
                    let (x, y) = (flat[i * n + t], flat[j * n + t]);
                    (canvas.x((x - lo) / (hi - lo)), canvas.y(y, lo, hi))
                })
                .collect();
            canvas.path(&pts, STROKE, colour);
            if self.points > 0.0 && pts.len() <= DOTS_MAX {
                for p in &pts {
                    canvas.stamp(p.0, p.1, self.points, colour);
                }
            }
            if let Some(head) = pts.iter().rev().find(|p| p.0.is_finite() && p.1.is_finite()) {
                canvas.stamp(head.0, head.1, self.points.max(HEAD), colour);
            }
        }
        Some(canvas.upload())
    }
}

/// Every `i < j` row pair — one trajectory each, row `i` across and row `j` up.
fn pairs(rows: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for i in 0..rows {
        for j in i + 1..rows {
            out.push((i, j));
            if out.len() >= MAX_PAIRS {
                return out;
            }
        }
    }
    out
}

/// A frame as RGBA texels, unclamped. `[N]` is one row; `[H, W]` is gray; `[H, W, C]` fills the
/// channels it has, with alpha 1 where it has none.
fn texels(frame: &Data) -> Option<Upload> {
    let Value::Array(a) = frame.value() else { return None };
    let (h, w, c) = match *a.shape() {
        [n] => (1, n, 1),
        [h, w] => (h, w, 1),
        [h, w, c] if (1..=4).contains(&c) => (h, w, c),
        _ => return None,
    };
    // A texture the device cannot make invalidates the whole frame's command buffer, so a frame
    // past the limit is no upload at all.
    if h == 0 || w == 0 || h > crate::plan::MAX_SIZE as usize || w > crate::plan::MAX_SIZE as usize {
        return None;
    }
    let x = floats(a.as_bytes());
    let mut texels = Vec::with_capacity(h * w * 4);
    for i in 0..h * w {
        let s = &x[i * c..(i + 1) * c];
        let rgba = match c {
            1 => [s[0], s[0], s[0], 1.0],
            2 => [s[0], s[0], s[0], s[1]],
            3 => [s[0], s[1], s[2], 1.0],
            _ => [s[0], s[1], s[2], s[3]],
        };
        texels.extend(rgba.iter().map(|v| f16(*v)));
    }
    Some(Upload { width: w as u32, height: h as u32, texels })
}

fn f16(v: f32) -> u16 {
    half::f16::from_f32(if v.is_finite() { v } else { 0.0 }).to_bits()
}

/// The texels a drawing is made on: straight alpha over transparent ground, row 0 the top.
struct Canvas {
    w: usize,
    h: usize,
    rgba: Vec<f32>,
}

impl Canvas {
    fn new((w, h): (u32, u32)) -> Canvas {
        let (w, h) = (w.max(1) as usize, h.max(1) as usize);
        Canvas { w, h, rgba: vec![0.0; w * h * 4] }
    }

    fn x(&self, t: f32) -> f32 {
        t * self.w as f32
    }

    fn y(&self, v: f32, lo: f32, hi: f32) -> f32 {
        self.h as f32 * (1.0 - (v - lo) / (hi - lo))
    }

    /// A round brush, blended by how much of the texel it covers — the whole anti-aliasing there is.
    fn stamp(&mut self, x: f32, y: f32, r: f32, colour: [f32; 3]) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        let reach = r + 0.5;
        let lo = |v: f32| v.max(0.0) as usize;
        let (x0, x1) = (lo(x - reach), (x + reach).max(0.0) as usize);
        let (y0, y1) = (lo(y - reach), (y + reach).max(0.0) as usize);
        for py in y0..=y1.min(self.h.saturating_sub(1)) {
            for px in x0..=x1.min(self.w.saturating_sub(1)) {
                let (dx, dy) = (px as f32 + 0.5 - x, py as f32 + 0.5 - y);
                let cover = (reach - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
                if cover > 0.0 {
                    let at = (py * self.w + px) * 4;
                    for (k, ink) in colour.iter().enumerate() {
                        self.rgba[at + k] = self.rgba[at + k] * (1.0 - cover) + ink * cover;
                    }
                    self.rgba[at + 3] = self.rgba[at + 3].max(cover);
                }
            }
        }
    }

    /// A polyline, broken wherever a point is not finite.
    fn path(&mut self, pts: &[(f32, f32)], r: f32, colour: [f32; 3]) {
        if pts.len() == 1 {
            self.stamp(pts[0].0, pts[0].1, r, colour);
        }
        for pair in pts.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let finite = a.0.is_finite() && a.1.is_finite() && b.0.is_finite() && b.1.is_finite();
            if let Some((a, b)) = finite.then(|| self.clip(a, b, r)).flatten() {
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let steps = ((dx * dx + dy * dy).sqrt() * 2.0).ceil().max(1.0) as usize;
                for s in 0..=steps {
                    let t = s as f32 / steps as f32;
                    self.stamp(a.0 + dx * t, a.1 + dy * t, r, colour);
                }
            }
        }
    }

    /// The part of a segment that can touch the canvas, or nothing: a value far outside the range
    /// would otherwise walk a million texels nobody sees.
    fn clip(&self, a: (f32, f32), b: (f32, f32), r: f32) -> Option<((f32, f32), (f32, f32))> {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        let edges = [
            (-dx, a.0 + r),
            (dx, self.w as f32 + r - a.0),
            (-dy, a.1 + r),
            (dy, self.h as f32 + r - a.1),
        ];
        for (p, q) in edges {
            if p == 0.0 {
                if q < 0.0 {
                    return None;
                }
                continue;
            }
            let t = q / p;
            if p < 0.0 {
                if t > t1 {
                    return None;
                }
                t0 = t0.max(t);
            } else {
                if t < t0 {
                    return None;
                }
                t1 = t1.min(t);
            }
        }
        Some(((a.0 + dx * t0, a.1 + dy * t0), (a.0 + dx * t1, a.1 + dy * t1)))
    }

    fn upload(self) -> Upload {
        Upload {
            width: self.w as u32,
            height: self.h as u32,
            texels: self.rgba.iter().map(|v| f16(*v)).collect(),
        }
    }
}
