//! Vectorize: an image as vector shapes, traced the way delyrism's symbol vectorizer traces a
//! drawing. Colour regions or ink layers are outlined and fitted with cubic Béziers, or a drawing's
//! pen paths become centre lines. Every frame is traced afresh and drawn back from its own curves.

use std::sync::OnceLock;

use goofi_core::{Data, Meta, SlotType};
use goofi_sdk::signal::{Face, Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, Params, ParamSpec, Show, SlotDecl, Tag};

const G: &str = "vectorize";
const REGIONS: Show = Show { param: "mode", any_of: &["regions"] };
const INK: Show = Show { param: "mode", any_of: &["ink", "lines"] };
const LINES: Show = Show { param: "mode", any_of: &["lines"] };

/// The label of a pixel that holds no ink.
const PAPER: u8 = 255;

type P = [f32; 2];
type Seg = [P; 4];
type Lab = [f32; 3];

fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}
fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1]]
}
fn mul(a: P, s: f32) -> P {
    [a[0] * s, a[1] * s]
}
fn dot(a: P, b: P) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}
fn norm(a: P) -> f32 {
    dot(a, a).sqrt()
}
fn unit(a: P) -> P {
    let n = norm(a);
    if n > 1e-9 {
        mul(a, 1.0 / n)
    } else {
        a
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Regions,
    Ink,
    Lines,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    pub size: usize,
    pub colors: usize,
    pub resolution: usize,
    pub detail: f32,
    pub smooth: f32,
    pub speckle: f32,
    pub chroma: f32,
    pub dark: f32,
    pub merge: f32,
    pub fill: f32,
    pub weight: f32,
}

fn config(p: &Params<'_>) -> Config {
    let num = |name: &str, fallback: f64, lo: f64, hi: f64| p.f64(G, name).unwrap_or(fallback).clamp(lo, hi) as f32;
    let int = |name: &str, fallback: i64, lo: i64, hi: i64| p.i64(G, name).unwrap_or(fallback).clamp(lo, hi) as usize;
    Config {
        mode: match p.str(G, "mode").unwrap_or("regions") {
            "ink" => Mode::Ink,
            "lines" => Mode::Lines,
            _ => Mode::Regions,
        },
        size: int("size", 512, 32, 4096),
        colors: int("colors", 8, 1, 32),
        resolution: int("resolution", 0, 0, 8192),
        detail: num("detail", 0.8, 0.05, 8.0),
        smooth: num("smooth", 1.2, 0.0, 6.0),
        speckle: num("speckle", 16.0, 0.0, 10000.0),
        chroma: num("chroma", 0.25, 0.0, 1.0),
        dark: num("dark", 0.40, 0.0, 1.0),
        merge: num("merge", 22.0, 0.0, 100.0),
        fill: num("fill", 1.8, 1.0, 6.0),
        weight: num("weight", 1.0, 0.0, 8.0),
    }
}

// ------------------------------------------------------------------------------------------------
//  The working picture and its colour
// ------------------------------------------------------------------------------------------------

pub struct Picture {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<[f32; 3]>,
}

/// The frame at the working size: the mean over each working pixel's footprint, read at no more
/// than four texels a side, with alpha laid over white paper.
pub fn picture(shape: &[usize], bytes: &[u8], size: usize) -> Result<(Picture, usize, usize), String> {
    let (h, w, c) = match *shape {
        [h, w] => (h, w, 1),
        [h, w, c] if (1..=4).contains(&c) => (h, w, c),
        ref s => return Err(format!("`image` takes [H, W] or [H, W, C] with C from 1 to 4, not {s:?}")),
    };
    if h == 0 || w == 0 || bytes.len() < h * w * c * 4 {
        return Err("`image` is empty".into());
    }
    let sc = (size as f64 / w.max(h) as f64).min(1.0);
    let (pw, ph) = (((w as f64 * sc).round() as usize).max(1), ((h as f64 * sc).round() as usize).max(1));
    let span = |n: usize, m: usize| -> Vec<(usize, usize, usize)> {
        (0..m)
            .map(|o| {
                let a = o * n / m;
                let b = ((o + 1) * n / m).max(a + 1);
                (a, b, ((b - a) / 4).max(1))
            })
            .collect()
    };
    let (xs, ys) = (span(w, pw), span(h, ph));
    let read = |i: usize| -> f32 {
        let v = f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("four bytes"));
        if v.is_finite() {
            v.clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    let texel = |at: usize| -> [f32; 3] {
        match c {
            1 => [read(at); 3],
            2 => [read(at) * read(at + 1) + 1.0 - read(at + 1); 3],
            3 => [read(at), read(at + 1), read(at + 2)],
            _ => {
                let a = read(at + 3);
                [read(at) * a + 1.0 - a, read(at + 1) * a + 1.0 - a, read(at + 2) * a + 1.0 - a]
            }
        }
    };
    let mut rgb = vec![[0f32; 3]; pw * ph];
    par_fill(&mut rgb, pw, |row0, part| {
        for (k, o) in part.iter_mut().enumerate() {
            let ((y0, y1, sy), (x0, x1, sx)) = (ys[row0 + k / pw], xs[k % pw]);
            let (mut acc, mut n) = ([0f32; 3], 0f32);
            for y in (y0..y1).step_by(sy) {
                for x in (x0..x1).step_by(sx) {
                    let t = texel((y * w + x) * c);
                    acc = [acc[0] + t[0], acc[1] + t[1], acc[2] + t[2]];
                    n += 1.0;
                }
            }
            *o = [acc[0] / n, acc[1] / n, acc[2] / n];
        }
    });
    Ok((Picture { w: pw, h: ph, rgb }, w, h))
}

struct Luts {
    linear: Vec<f32>,
    f: Vec<f32>,
}

const F_MAX: f32 = 1.1;

fn luts() -> &'static Luts {
    static L: OnceLock<Luts> = OnceLock::new();
    L.get_or_init(|| {
        let linear = (0..1024)
            .map(|i| {
                let v = i as f32 / 1023.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            })
            .collect();
        let d = 6.0f32 / 29.0;
        let f = (0..2048)
            .map(|i| {
                let t = i as f32 / 2047.0 * F_MAX;
                if t > d * d * d {
                    t.cbrt()
                } else {
                    t / (3.0 * d * d) + 4.0 / 29.0
                }
            })
            .collect();
        Luts { linear, f }
    })
}

fn lookup(t: &[f32], v: f32, scale: f32) -> f32 {
    let x = (v * scale).clamp(0.0, (t.len() - 1) as f32);
    let i = (x as usize).min(t.len() - 2);
    t[i] + (t[i + 1] - t[i]) * (x - i as f32)
}

/// CIE Lab of sRGB values, D65.
fn to_lab(rgb: &[[f32; 3]], w: usize) -> Vec<Lab> {
    let l = luts();
    let (ls, fs) = (1023.0, 2047.0 / F_MAX);
    let mut out = vec![[0f32; 3]; rgb.len()];
    par_fill(&mut out, w, |row0, part| {
        for (o, p) in part.iter_mut().zip(&rgb[row0 * w..]) {
            let (r, g, b) = (lookup(&l.linear, p[0], ls), lookup(&l.linear, p[1], ls), lookup(&l.linear, p[2], ls));
            let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
            let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
            let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
            let (fx, fy, fz) = (lookup(&l.f, x, fs), lookup(&l.f, y, fs), lookup(&l.f, z, fs));
            *o = [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)];
        }
    });
    out
}

fn d2(a: Lab, b: Lab) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

fn nearest(c: &[Lab], p: Lab) -> usize {
    let mut best = (f32::MAX, 0);
    for (j, &q) in c.iter().enumerate() {
        let d = d2(p, q);
        if d < best.0 {
            best = (d, j);
        }
    }
    best.1
}

/// The ink-colour distance of delyrism's palette: one marker varies in lightness and saturation
/// with pressure and overlap and keeps its hue, so for chromatic pairs lightness and chroma count
/// less than hue; near-grey pairs use plain ΔE.
fn ink_dist(a: Lab, b: Lab) -> f32 {
    let dl = a[0] - b[0];
    let (ca, cb) = (a[1].hypot(a[2]), b[1].hypot(b[2]));
    let (ha, hb) = (a[2].atan2(a[1]), b[2].atan2(b[1]));
    let tau = std::f32::consts::TAU;
    let dh = ((ha - hb + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI).abs();
    let chromatic = (0.35 * dl).hypot((0.5 * (ca - cb)).hypot(dh * ca.min(cb)));
    let plain = d2(a, b).sqrt();
    let w = ((ca.min(cb) - 8.0) / 10.0).clamp(0.0, 1.0);
    w * chromatic + (1.0 - w) * plain
}

struct Rng(u64);

impl Rng {
    fn unit(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// At most `n` points of `xs`, evenly strided.
fn sample<T: Copy>(xs: &[T], n: usize) -> Vec<T> {
    let step = (xs.len() / n.max(1)).max(1);
    xs.iter().step_by(step).copied().collect()
}

/// k-means in Lab, started from `warm` (the last frame's palette) and filled up by k-means++. A
/// centre that loses every point moves to the point farthest from its own centre.
fn kmeans(x: &[Lab], k: usize, warm: &[Lab], iters: usize) -> Vec<Lab> {
    let k = k.min(x.len()).max(1);
    let mut c: Vec<Lab> = warm.iter().take(k).copied().collect();
    if c.is_empty() {
        c.push(x[x.len() / 2]);
    }
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut dist: Vec<f32> = x.iter().map(|&p| c.iter().map(|&q| d2(p, q)).fold(f32::MAX, f32::min)).collect();
    while c.len() < k {
        let total: f64 = dist.iter().map(|&d| d as f64).sum();
        if total <= 0.0 {
            break;
        }
        let (mut r, mut pick) = (rng.unit() * total, x.len() - 1);
        for (i, &d) in dist.iter().enumerate() {
            r -= d as f64;
            if r <= 0.0 {
                pick = i;
                break;
            }
        }
        let q = x[pick];
        c.push(q);
        for (i, &p) in x.iter().enumerate() {
            dist[i] = dist[i].min(d2(p, q));
        }
    }
    let k = c.len();
    let (mut sum, mut cnt) = (vec![[0f64; 3]; k], vec![0usize; k]);
    let mut own = vec![0usize; x.len()];
    for _ in 0..iters {
        sum.fill([0.0; 3]);
        cnt.fill(0);
        for (i, &p) in x.iter().enumerate() {
            let j = nearest(&c, p);
            own[i] = j;
            for d in 0..3 {
                sum[j][d] += p[d] as f64;
            }
            cnt[j] += 1;
        }
        let mut moved = 0f32;
        for j in 0..k {
            let m = if cnt[j] > 0 {
                [(sum[j][0] / cnt[j] as f64) as f32, (sum[j][1] / cnt[j] as f64) as f32, (sum[j][2] / cnt[j] as f64) as f32]
            } else {
                let far = (0..x.len()).max_by(|&a, &b| d2(x[a], c[own[a]]).total_cmp(&d2(x[b], c[own[b]]))).unwrap_or(0);
                x[far]
            };
            moved = moved.max(d2(m, c[j]));
            c[j] = m;
        }
        if moved < 0.01 {
            break;
        }
    }
    c
}

// ------------------------------------------------------------------------------------------------
//  Pixel maps: components, morphology, distance, thinning
// ------------------------------------------------------------------------------------------------

/// Connected pixels of equal value: each pixel's component, and per component its value, area,
/// first pixel in raster order and bounding box (x0, y0, x1, y1, inclusive). `runs` are the
/// horizontal runs of one value, row by row (`row` indexes each row's first run): the unit every
/// pass over components works in.
struct Comps {
    map: Vec<u32>,
    value: Vec<u8>,
    area: Vec<u32>,
    first: Vec<u32>,
    bbox: Vec<[u32; 4]>,
    runs: Vec<Run>,
    row: Vec<usize>,
}

#[derive(Clone, Copy)]
struct Run {
    x0: u32,
    x1: u32,
    comp: u32,
}

fn components(v: &[u8], w: usize, h: usize, conn8: bool) -> Comps {
    fn find(p: &mut [u32], mut a: u32) -> u32 {
        while p[a as usize] != a {
            p[a as usize] = p[p[a as usize] as usize];
            a = p[a as usize];
        }
        a
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut row = Vec::with_capacity(h + 1);
    let mut parent: Vec<u32> = Vec::new();
    let reach = conn8 as u32;
    for y in 0..h {
        let line = &v[y * w..(y + 1) * w];
        let above = if y > 0 { row[y - 1]..runs.len() } else { 0..0 };
        row.push(runs.len());
        let mut x = 0;
        let mut k = above.start;
        while x < w {
            let val = line[x];
            let x0 = x;
            while x < w && line[x] == val {
                x += 1;
            }
            let id = runs.len() as u32;
            parent.push(id);
            let (lo, hi) = (x0 as u32, x as u32);
            // Runs above that touch this one: overlap, or meet at a corner when diagonals count.
            while k < above.end && runs[k].x1 + reach <= lo {
                k += 1;
            }
            let mut j = k;
            while j < above.end && runs[j].x0 < hi + reach {
                if v[(y - 1) * w + runs[j].x0 as usize] == val {
                    let (a, b) = (find(&mut parent, id), find(&mut parent, j as u32));
                    if a != b {
                        parent[a.max(b) as usize] = a.min(b);
                    }
                }
                j += 1;
            }
            runs.push(Run { x0: lo, x1: hi, comp: 0 });
        }
    }
    row.push(runs.len());
    let mut id = vec![u32::MAX; runs.len()];
    let mut c = Comps { map: vec![0; w * h], value: vec![], area: vec![], first: vec![], bbox: vec![], runs: vec![], row: vec![] };
    for y in 0..h {
        for (r, run) in runs.iter_mut().enumerate().take(row[y + 1]).skip(row[y]) {
            let root = find(&mut parent, r as u32) as usize;
            let Run { x0, x1, .. } = *run;
            if id[root] == u32::MAX {
                id[root] = c.area.len() as u32;
                c.value.push(v[y * w + x0 as usize]);
                c.area.push(0);
                c.first.push((y * w) as u32 + x0);
                c.bbox.push([x0, y as u32, x1 - 1, y as u32]);
            }
            let k = id[root];
            run.comp = k;
            let k = k as usize;
            c.area[k] += x1 - x0;
            let b = &mut c.bbox[k];
            *b = [b[0].min(x0), b[1], b[2].max(x1 - 1), y as u32];
            c.map[y * w + x0 as usize..y * w + x1 as usize].fill(k as u32);
        }
    }
    c.runs = runs;
    c.row = row;
    c
}

/// Components smaller than `min_area` take the value they share the longest border with, smallest
/// first, so specks and anti-aliasing seams join the region around them. Paper is never merged
/// and never absorbs.
fn merge_small(labels: &mut [u8], w: usize, h: usize, min_area: f32) {
    if min_area <= 1.0 {
        return;
    }
    let comps = components(labels, w, h, false);
    let n = comps.area.len();
    let small = |c: usize| (comps.area[c] as f32) < min_area && comps.value[c] != PAPER;
    let mut order: Vec<usize> = (0..n).filter(|&c| small(c)).collect();
    if order.is_empty() {
        return;
    }
    // Shared border lengths, read off the runs: a run meets the next one in its row over one
    // pixel, and a run in the row below over the columns they share.
    let mut edges: Vec<(u32, u32, u32)> = Vec::new();
    let mut meet = |a: u32, b: u32, len: u32| {
        if a != b {
            if small(a as usize) {
                edges.push((a, b, len));
            }
            if small(b as usize) {
                edges.push((b, a, len));
            }
        }
    };
    let runs = &comps.runs;
    for y in 0..h {
        let (s, e) = (comps.row[y], comps.row[y + 1]);
        for r in s + 1..e {
            meet(runs[r - 1].comp, runs[r].comp, 1);
        }
        if y + 1 < h {
            let (mut j, e2) = (e, comps.row[y + 2]);
            for r in s..e {
                while j < e2 && runs[j].x1 <= runs[r].x0 {
                    j += 1;
                }
                let mut q = j;
                while q < e2 && runs[q].x0 < runs[r].x1 {
                    let len = runs[q].x1.min(runs[r].x1) - runs[q].x0.max(runs[r].x0);
                    meet(runs[r].comp, runs[q].comp, len);
                    q += 1;
                }
            }
        }
    }
    // Grouped by their small component, by counting.
    let mut at = vec![0u32; n + 1];
    for e in &edges {
        at[e.0 as usize + 1] += 1;
    }
    for c in 0..n {
        at[c + 1] += at[c];
    }
    let mut fill = at.clone();
    let mut grouped = vec![(0u32, 0u32, 0u32); edges.len()];
    for &e in &edges {
        grouped[fill[e.0 as usize] as usize] = e;
        fill[e.0 as usize] += 1;
    }
    let edges = grouped;
    order.sort_by_key(|&c| comps.area[c]);
    let mut parent: Vec<u32> = (0..n as u32).collect();
    let mut area = comps.area.clone();
    // The components merged into each one, as a chain: their borders are its borders now.
    let (mut next, mut last): (Vec<u32>, Vec<u32>) = (vec![u32::MAX; n], (0..n as u32).collect());
    fn find(parent: &mut [u32], mut c: u32) -> u32 {
        while parent[c as usize] != c {
            parent[c as usize] = parent[parent[c as usize] as usize];
            c = parent[c as usize];
        }
        c
    }
    let mut tally: Vec<(u32, u32)> = Vec::new();
    for c in order {
        if (area[c] as f32) >= min_area {
            continue;
        }
        tally.clear();
        let mut m = c as u32;
        while m != u32::MAX {
            for &(_, b, len) in &edges[at[m as usize] as usize..at[m as usize + 1] as usize] {
                let r = find(&mut parent, b);
                if r as usize != c && comps.value[r as usize] != PAPER {
                    tally.push((r, len));
                }
            }
            m = next[m as usize];
        }
        tally.sort_unstable_by_key(|t| t.0);
        let (mut best, mut most, mut k) = (None, 0, 0);
        while k < tally.len() {
            let (r, mut sum) = (tally[k].0, 0);
            while k < tally.len() && tally[k].0 == r {
                sum += tally[k].1;
                k += 1;
            }
            if sum > most {
                (best, most) = (Some(r as usize), sum);
            }
        }
        let Some(t) = best else { continue };
        parent[c] = t as u32;
        area[t] += area[c];
        next[last[t] as usize] = c as u32;
        last[t] = last[c];
    }
    let value: Vec<u8> = (0..n as u32).map(|c| comps.value[find(&mut parent, c) as usize]).collect();
    for y in 0..h {
        for r in &runs[comps.row[y]..comps.row[y + 1]] {
            labels[y * w + r.x0 as usize..y * w + r.x1 as usize].fill(value[r.comp as usize]);
        }
    }
}

/// A pixel whose label no more than one of its eight neighbours shares takes the label at least
/// five of them share: single-pixel noise goes before the regions are counted. Paper never wins
/// and never moves.
fn majority(labels: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = labels.to_vec();
    par_fill(&mut out, w, |row0, part| {
        for (k, o) in part.iter_mut().enumerate() {
            let i = row0 * w + k;
            let (x, y) = (i % w, i / w);
            if x == 0 || y == 0 || x + 1 == w || y + 1 == h || labels[i] == PAPER {
                continue;
            }
            let n = [labels[i - w - 1], labels[i - w], labels[i - w + 1], labels[i - 1], labels[i + 1], labels[i + w - 1], labels[i + w], labels[i + w + 1]];
            if n.iter().filter(|&&l| l == labels[i]).count() >= 2 {
                continue;
            }
            for &cand in &n {
                if cand != PAPER && n.iter().filter(|&&l| l == cand).count() >= 5 {
                    *o = cand;
                    break;
                }
            }
        }
    });
    out
}

/// Fills `out` in row bands on the machine's cores: `f` gets a band's first row and its slice.
fn par_fill<T: Send>(out: &mut [T], row_len: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let rows = out.len() / row_len.max(1);
    let threads = cores().min(rows.max(1));
    if threads < 2 || out.len() < 32768 {
        f(0, out);
        return;
    }
    let band = rows.div_ceil(threads);
    let f = &f;
    std::thread::scope(|s| {
        for (k, part) in out.chunks_mut(band * row_len).enumerate() {
            s.spawn(move || f(k * band, part));
        }
    });
}

fn cores() -> usize {
    static N: OnceLock<usize> = OnceLock::new();
    *N.get_or_init(|| std::thread::available_parallelism().map_or(1, |n| n.get()).min(8))
}

const SQUARE: &[(i32, i32)] = &[(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];
const CROSS: &[(i32, i32)] = &[(0, -1), (-1, 0), (1, 0), (0, 1)];

/// Erosion (a set pixel stays only if every neighbour in `k` is set) or dilation (every
/// neighbour of a set pixel sets). Both visit set pixels only; outside the frame never erodes.
fn morph(m: &[bool], w: usize, h: usize, k: &[(i32, i32)], erode: bool) -> Vec<bool> {
    let mut out = m.to_vec();
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let i = y as usize * w + x as usize;
            if !m[i] {
                continue;
            }
            for &(dx, dy) in k {
                let (u, v) = (x + dx, y + dy);
                if u < 0 || v < 0 || u >= w as i32 || v >= h as i32 {
                    continue;
                }
                let j = v as usize * w + u as usize;
                if !erode {
                    out[j] = true;
                } else if !m[j] {
                    out[i] = false;
                    break;
                }
            }
        }
    }
    out
}

/// Chamfer distance (1, √2) from each set pixel to the nearest unset one, within the rectangle
/// `r` (x0, y0, x1, y1, exclusive); with `seed`, also the label of the nearest seeded pixel.
fn chamfer(dist: &mut [f32], mut seed: Option<&mut [u8]>, w: usize, r: [usize; 4]) {
    const R2: f32 = std::f32::consts::SQRT_2;
    let mut relax = |i: usize, j: usize, c: f32| {
        if dist[j] + c < dist[i] {
            dist[i] = dist[j] + c;
            if let Some(s) = seed.as_deref_mut() {
                s[i] = s[j];
            }
        }
    };
    let [x0, y0, x1, y1] = r;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = y * w + x;
            if x > x0 {
                relax(i, i - 1, 1.0);
            }
            if y > y0 {
                relax(i, i - w, 1.0);
                if x > x0 {
                    relax(i, i - w - 1, R2);
                }
                if x + 1 < x1 {
                    relax(i, i - w + 1, R2);
                }
            }
        }
    }
    for y in (y0..y1).rev() {
        for x in (x0..x1).rev() {
            let i = y * w + x;
            if x + 1 < x1 {
                relax(i, i + 1, 1.0);
            }
            if y + 1 < y1 {
                relax(i, i + w, 1.0);
                if x + 1 < x1 {
                    relax(i, i + w + 1, R2);
                }
                if x > x0 {
                    relax(i, i + w - 1, R2);
                }
            }
        }
    }
}

/// The rectangle around the set pixels, one pixel wider on each side and inside the frame.
fn around(m: &[bool], w: usize, h: usize) -> [usize; 4] {
    let mut r = [usize::MAX, usize::MAX, 0, 0];
    for y in 0..h {
        let row = &m[y * w..(y + 1) * w];
        let (Some(a), Some(b)) = (row.iter().position(|&v| v), row.iter().rposition(|&v| v)) else { continue };
        r = [r[0].min(a), r[1].min(y), r[2].max(b), y];
    }
    if r[0] == usize::MAX {
        return [0, 0, 0, 0];
    }
    [r[0].saturating_sub(1), r[1].saturating_sub(1), (r[2] + 2).min(w), (r[3] + 2).min(h)]
}

fn distance(m: &[bool], w: usize, h: usize) -> Vec<f32> {
    let mut d: Vec<f32> = m.iter().map(|&s| if s { 1e9 } else { 0.0 }).collect();
    chamfer(&mut d, None, w, around(m, w, h));
    d
}

/// Every pixel of `within` takes the label of its nearest seed (a label other than `PAPER`).
fn nearest_seed(seed: &mut [u8], within: &[bool], w: usize, h: usize) {
    let mut d: Vec<f32> = seed.iter().map(|&s| if s != PAPER { 0.0 } else { 1e9 }).collect();
    chamfer(&mut d, Some(seed), w, around(within, w, h));
}

/// Zhang–Suen thinning, the method delyrism's tracer uses through OpenCV.
fn thin(m: &[bool], w: usize, h: usize) -> Vec<bool> {
    let (pw, ph) = (w + 2, h + 2);
    let mut img = vec![0u8; pw * ph];
    let mut active = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if m[y * w + x] {
                let i = (y + 1) * pw + x + 1;
                img[i] = 1;
                active.push(i);
            }
        }
    }
    let mut del = Vec::new();
    loop {
        let mut changed = false;
        for step in 0..2 {
            del.clear();
            for &i in &active {
                if img[i] == 0 {
                    continue;
                }
                let n = [img[i - pw], img[i - pw + 1], img[i + 1], img[i + pw + 1], img[i + pw], img[i + pw - 1], img[i - 1], img[i - pw - 1]];
                let b: u8 = n.iter().sum();
                if !(2..=6).contains(&b) {
                    continue;
                }
                let a = (0..8).filter(|&k| n[k] == 0 && n[(k + 1) % 8] == 1).count();
                if a != 1 {
                    continue;
                }
                let (p2, p4, p6, p8) = (n[0], n[2], n[4], n[6]);
                let ok = if step == 0 { p2 * p4 * p6 == 0 && p4 * p6 * p8 == 0 } else { p2 * p4 * p8 == 0 && p2 * p6 * p8 == 0 };
                if ok {
                    del.push(i);
                }
            }
            for &i in &del {
                img[i] = 0;
            }
            changed |= !del.is_empty();
        }
        active.retain(|&i| img[i] == 1);
        if !changed {
            break;
        }
    }
    let mut out = vec![false; w * h];
    for &i in &active {
        out[(i / pw - 1) * w + i % pw - 1] = true;
    }
    out
}

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    let k = v.len() / 2;
    *v.select_nth_unstable_by(k, f32::total_cmp).1
}

// ------------------------------------------------------------------------------------------------
//  Curves: Schneider's cubic fitting, split at corners
// ------------------------------------------------------------------------------------------------

fn bez(c: &Seg, t: f32) -> P {
    let mt = 1.0 - t;
    let (a, b, cc, d) = (mt * mt * mt, 3.0 * mt * mt * t, 3.0 * mt * t * t, t * t * t);
    [a * c[0][0] + b * c[1][0] + cc * c[2][0] + d * c[3][0], a * c[0][1] + b * c[1][1] + cc * c[2][1] + d * c[3][1]]
}

fn bez_d(c: &Seg, t: f32) -> P {
    let mt = 1.0 - t;
    add(add(mul(sub(c[1], c[0]), 3.0 * mt * mt), mul(sub(c[2], c[1]), 6.0 * mt * t)), mul(sub(c[3], c[2]), 3.0 * t * t))
}

fn bez_dd(c: &Seg, t: f32) -> P {
    let a = add(sub(c[2], mul(c[1], 2.0)), c[0]);
    let b = add(sub(c[3], mul(c[2], 2.0)), c[1]);
    add(mul(a, 6.0 * (1.0 - t)), mul(b, 6.0 * t))
}

fn fit_one(p: &[P], u: &[f32], t1: P, t2: P) -> Seg {
    let (p0, pn) = (p[0], p[p.len() - 1]);
    let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0f64, 0f64, 0f64, 0f64, 0f64);
    let t12 = dot(t1, t2) as f64;
    for (q, &t) in p.iter().zip(u) {
        let mt = 1.0 - t;
        let (b1, b2) = (3.0 * mt * mt * t, 3.0 * mt * t * t);
        let base = add(mul(p0, mt * mt * mt + b1), mul(pn, b2 + t * t * t));
        let tmp = sub(*q, base);
        c00 += (b1 * b1) as f64;
        c01 += (b1 * b2) as f64 * t12;
        c11 += (b2 * b2) as f64;
        x0 += (b1 * dot(t1, tmp)) as f64;
        x1 += (b2 * dot(t2, tmp)) as f64;
    }
    let det = c00 * c11 - c01 * c01;
    let seg = norm(sub(pn, p0));
    let (mut a1, mut a2) = if det.abs() > 1e-12 {
        (((x0 * c11 - x1 * c01) / det) as f32, ((c00 * x1 - c01 * x0) / det) as f32)
    } else {
        (seg / 3.0, seg / 3.0)
    };
    if a1 < 1e-6 * seg || a2 < 1e-6 * seg {
        (a1, a2) = (seg / 3.0, seg / 3.0);
    }
    [p0, add(p0, mul(t1, a1)), add(pn, mul(t2, a2)), pn]
}

fn fit(p: &[P], err: f32, t1: Option<P>, t2: Option<P>, depth: u32, out: &mut Vec<Seg>) {
    let n = p.len();
    if n < 2 {
        return;
    }
    let t1 = t1.unwrap_or_else(|| unit(sub(p[3.min(n - 1)], p[0])));
    let t2 = t2.unwrap_or_else(|| unit(sub(p[n - 4.min(n)], p[n - 1])));
    if n == 2 || (norm(sub(p[n - 1], p[0])) < 1e-6 && n < 4) {
        let d = norm(sub(p[n - 1], p[0])) / 3.0;
        out.push([p[0], add(p[0], mul(t1, d)), add(p[n - 1], mul(t2, d)), p[n - 1]]);
        return;
    }
    let mut u: Vec<f32> = Vec::with_capacity(n);
    let mut acc = 0.0;
    u.push(0.0);
    for k in 1..n {
        acc += norm(sub(p[k], p[k - 1]));
        u.push(acc);
    }
    if acc > 0.0 {
        u.iter_mut().for_each(|v| *v /= acc);
    } else {
        u.iter_mut().enumerate().for_each(|(k, v)| *v = k as f32 / (n - 1) as f32);
    }
    let err2 = err * err;
    let mut ctrl = fit_one(p, &u, t1, t2);
    let (mut worst, mut at) = (0f32, 0usize);
    for round in 0..5 {
        (worst, at) = (0.0, 0);
        for k in 0..n {
            let e = sub(bez(&ctrl, u[k]), p[k]);
            let e = dot(e, e);
            if e > worst {
                (worst, at) = (e, k);
            }
        }
        if worst <= err2 || round == 4 {
            break;
        }
        for k in 0..n {
            let q = sub(bez(&ctrl, u[k]), p[k]);
            let (d1, dd) = (bez_d(&ctrl, u[k]), bez_dd(&ctrl, u[k]));
            let den = dot(d1, d1) + dot(q, dd);
            if den.abs() > 1e-9 {
                u[k] = (u[k] - dot(q, d1) / den).clamp(0.0, 1.0);
            }
        }
        ctrl = fit_one(p, &u, t1, t2);
    }
    if worst <= err2 || n < 6 || depth > 40 {
        out.push(ctrl);
        return;
    }
    let k = at.clamp(2, n - 3);
    let tc = unit(sub(p[k - 1], p[k + 1]));
    fit(&p[..=k], err, Some(t1), Some(tc), depth + 1, out);
    fit(&p[k..], err, Some(mul(tc, -1.0)), Some(t2), depth + 1, out);
}

/// Indices where the polyline turns sharper than `angle` degrees over ±k points.
fn corners(p: &[P], closed: bool, k: usize, angle: f32) -> Vec<usize> {
    let n = p.len();
    if n < 2 * k + 3 {
        return vec![];
    }
    let cos_max = angle.to_radians().cos();
    let mut sharp: Vec<(f32, usize)> = Vec::new();
    for i in 0..n {
        if !closed && (i <= k || i + k + 1 >= n) {
            continue;
        }
        let (a, b) = if closed { (p[(i + n - k) % n], p[(i + k) % n]) } else { (p[i - k], p[i + k]) };
        let (v1, v2) = (sub(p[i], a), sub(b, p[i]));
        let cos = dot(v1, v2) / (norm(v1) * norm(v2) + 1e-9);
        if cos <= cos_max {
            sharp.push((cos, i));
        }
    }
    sharp.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<usize> = Vec::new();
    for (_, i) in sharp {
        if out.iter().all(|&j| {
            let d = i.abs_diff(j);
            d.min(n - d) > 2 * k
        }) {
            out.push(i);
        }
    }
    out.sort_unstable();
    out
}

/// Cubic segments within `err` px of a polyline, split at its corners so they stay sharp.
fn beziers(p: &[P], closed: bool, err: f32) -> Vec<Seg> {
    let mut out = Vec::new();
    if p.len() < 2 {
        return out;
    }
    if closed {
        let p = if norm(sub(p[0], p[p.len() - 1])) < 1e-6 { &p[..p.len() - 1] } else { p };
        let n = p.len();
        if n < 3 {
            return out;
        }
        let cs = corners(p, true, 6, 55.0);
        if cs.is_empty() {
            let mut q = p.to_vec();
            q.push(p[0]);
            let t = unit(sub(p[1], p[n - 1]));
            fit(&q, err, Some(t), Some(mul(t, -1.0)), 0, &mut out);
            return out;
        }
        let mut q: Vec<P> = (0..=n).map(|i| p[(cs[0] + i) % n]).collect();
        q.truncate(n + 1);
        let mut cuts: Vec<usize> = cs.iter().map(|&c| (c + n - cs[0]) % n).collect();
        cuts.push(n);
        for w in cuts.windows(2) {
            fit(&q[w[0]..=w[1]], err, None, None, 0, &mut out);
        }
        return out;
    }
    let mut cuts = vec![0];
    cuts.extend(corners(p, false, 6, 55.0));
    cuts.push(p.len() - 1);
    for w in cuts.windows(2) {
        fit(&p[w[0]..=w[1]], err, None, None, 0, &mut out);
    }
    out
}

fn smooth_closed(p: &[P], sigma: f32) -> Vec<P> {
    let n = p.len();
    if sigma < 0.05 || n < 6 {
        return p.to_vec();
    }
    let r = ((3.0 * sigma).ceil() as usize).min((n - 1) / 2);
    let k: Vec<f32> = (0..=2 * r).map(|i| (-((i as f32 - r as f32).powi(2)) / (2.0 * sigma * sigma)).exp()).collect();
    let s: f32 = k.iter().sum();
    (0..n)
        .map(|i| {
            let mut acc = [0f32; 2];
            for (j, &kj) in k.iter().enumerate() {
                let q = p[(i + n + j - r) % n];
                acc = [acc[0] + kj * q[0], acc[1] + kj * q[1]];
            }
            [acc[0] / s, acc[1] / s]
        })
        .collect()
}

fn smooth_open(p: &[P], sigma: f32) -> Vec<P> {
    let n = p.len();
    if sigma < 0.05 || n < 5 {
        return p.to_vec();
    }
    let r = (3.0 * sigma).ceil() as i64;
    let k: Vec<f32> = (-r..=r).map(|i| (-((i * i) as f32) / (2.0 * sigma * sigma)).exp()).collect();
    let s: f32 = k.iter().sum();
    let mut q: Vec<P> = (0..n as i64)
        .map(|i| {
            let mut acc = [0f32; 2];
            for (j, &kj) in k.iter().enumerate() {
                let at = (i + j as i64 - r).clamp(0, n as i64 - 1) as usize;
                acc = [acc[0] + kj * p[at][0], acc[1] + kj * p[at][1]];
            }
            [acc[0] / s, acc[1] / s]
        })
        .collect();
    q[0] = p[0];
    q[n - 1] = p[n - 1];
    q
}

/// The polyline at `step` spacing along its length.
fn resample(p: &[P], step: f32) -> Vec<P> {
    let mut d = vec![0f32; p.len()];
    for k in 1..p.len() {
        d[k] = d[k - 1] + norm(sub(p[k], p[k - 1]));
    }
    let total = d[p.len() - 1];
    if total < 1e-6 {
        return p[..1].to_vec();
    }
    let m = ((total / step).ceil() as usize + 1).max(2);
    let mut out = Vec::with_capacity(m);
    let mut k = 0;
    for i in 0..m {
        let s = total * i as f32 / (m - 1) as f32;
        while k + 2 < p.len() && d[k + 1] < s {
            k += 1;
        }
        let span = (d[k + 1] - d[k]).max(1e-9);
        let t = ((s - d[k]) / span).clamp(0.0, 1.0);
        out.push(add(p[k], mul(sub(p[k + 1], p[k]), t)));
    }
    out
}

// ------------------------------------------------------------------------------------------------
//  The traced picture
// ------------------------------------------------------------------------------------------------

/// One colour's filled outline: rings of cubic segments in working px, y down, filled nonzero
/// (holes run the other way), and the same rings flattened for drawing.
pub struct Shape {
    pub color: usize,
    pub rings: Vec<Vec<Seg>>,
    pub polys: Vec<Vec<P>>,
}

pub struct Stroke {
    pub points: Vec<P>,
    pub color: usize,
    pub width: f32,
    pub closed: bool,
    pub dot: bool,
}

pub struct Vector {
    pub w: usize,
    pub h: usize,
    pub palette: Vec<[f32; 3]>,
    /// The palette entry that fills the whole frame under the shapes, in `regions`.
    pub ground: Option<usize>,
    pub shapes: Vec<Shape>,
    pub strokes: Vec<Stroke>,
}

/// The outer boundary of component `r` along pixel edges, the component on the right, so the
/// ring runs clockwise on screen; diagonal neighbours are apart (4-connectivity).
fn trace(map: &[u32], w: usize, h: usize, r: u32, start: u32) -> Vec<P> {
    let inside = |x: i32, y: i32| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && map[y as usize * w + x as usize] == r;
    const D: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
    // The pixels right and left of the edge that leaves vertex (x, y) in direction d.
    let sides = |x: i32, y: i32, d: usize| match d {
        0 => ((x, y), (x, y - 1)),
        1 => ((x - 1, y), (x, y)),
        2 => ((x - 1, y - 1), (x - 1, y)),
        _ => ((x, y - 1), (x - 1, y - 1)),
    };
    let (sx, sy) = ((start as usize % w) as i32, (start as usize / w) as i32);
    let (mut x, mut y, mut d) = (sx, sy, 0usize);
    let mut pts = Vec::new();
    loop {
        pts.push([x as f32, y as f32]);
        x += D[d].0;
        y += D[d].1;
        if x == sx && y == sy {
            break;
        }
        for nd in [(d + 1) % 4, d, (d + 3) % 4] {
            let ((rx, ry), (lx, ly)) = sides(x, y, nd);
            if inside(rx, ry) && !inside(lx, ly) {
                d = nd;
                break;
            }
        }
    }
    pts
}

fn area(p: &[P]) -> f32 {
    let n = p.len();
    (0..n).map(|i| p[i][0] * p[(i + 1) % n][1] - p[(i + 1) % n][0] * p[i][1]).sum::<f32>() * 0.5
}

/// A traced ring smoothed and fitted. Vertices on the frame's edge keep that coordinate, so a
/// shape that meets the edge still meets it.
fn fit_ring(pts: &[P], edges: [f32; 4], cfg: &Config) -> Vec<Seg> {
    let mut q = smooth_closed(pts, cfg.smooth);
    for (a, b) in q.iter_mut().zip(pts) {
        if b[0] == edges[0] || b[0] == edges[2] {
            a[0] = b[0];
        }
        if b[1] == edges[1] || b[1] == edges[3] {
            a[1] = b[1];
        }
    }
    beziers(&q, true, cfg.detail)
}

/// Points along a chain of cubic segments, `scale` output px per working px deciding how finely.
fn flatten(segs: &[Seg], scale: f32) -> Vec<P> {
    let mut out = Vec::new();
    for s in segs {
        let len = norm(sub(s[1], s[0])) + norm(sub(s[2], s[1])) + norm(sub(s[3], s[2]));
        let n = ((len * scale / 3.0).ceil() as usize).clamp(1, 24);
        if out.is_empty() {
            out.push(s[0]);
        }
        for k in 1..=n {
            out.push(bez(s, k as f32 / n as f32));
        }
    }
    out
}

/// Runs `f` over `items` on the machine's cores, in order. Scoped: every thread ends with the call.
fn par_map<T: Send, U: Send>(items: Vec<T>, f: impl Fn(T) -> U + Sync) -> Vec<U> {
    let threads = cores();
    if threads < 2 || items.len() < 64 {
        return items.into_iter().map(f).collect();
    }
    let chunk = items.len().div_ceil(threads);
    let mut groups: Vec<Vec<T>> = Vec::new();
    let mut it = items.into_iter();
    loop {
        let g: Vec<T> = it.by_ref().take(chunk).collect();
        if g.is_empty() {
            break;
        }
        groups.push(g);
    }
    let f = &f;
    std::thread::scope(|s| {
        let handles: Vec<_> = groups.into_iter().map(|g| s.spawn(move || g.into_iter().map(f).collect::<Vec<U>>())).collect();
        handles.into_iter().flat_map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).collect()
    })
}

/// The mean colour of each label's pixels.
fn mean_colors(rgb: &[[f32; 3]], labels: &[u8], k: usize, only: Option<&[bool]>) -> Vec<[f32; 3]> {
    let (mut sum, mut cnt) = (vec![[0f64; 3]; k], vec![0u32; k]);
    for (i, &l) in labels.iter().enumerate() {
        if (l as usize) < k && only.is_none_or(|m| m[i]) {
            for d in 0..3 {
                sum[l as usize][d] += rgb[i][d] as f64;
            }
            cnt[l as usize] += 1;
        }
    }
    (0..k).map(|j| if cnt[j] > 0 { [0, 1, 2].map(|d| (sum[j][d] / cnt[j] as f64) as f32) } else { [0.5; 3] }).collect()
}

/// `regions`: the picture posterized to a palette, every connected region of one colour traced.
/// Shapes are stacked by the area their outline encloses, largest first, so a region inside
/// another's hole is drawn over it and no seam opens between neighbours.
fn regions(pic: &Picture, cfg: &Config, warm: &mut Vec<Lab>, scale: f32) -> Vector {
    let (w, h) = (pic.w, pic.h);
    let lab = to_lab(&pic.rgb, w);
    let k = cfg.colors.max(2);
    let fresh = warm.len() != k;
    let centres = kmeans(&sample(&lab, 12000), k, if fresh { &[] } else { warm }, if fresh { 16 } else { 4 });
    warm.clone_from(&centres);
    let mut labels = vec![0u8; w * h];
    par_fill(&mut labels, w, |row0, part| {
        for (o, &p) in part.iter_mut().zip(&lab[row0 * w..]) {
            *o = nearest(&centres, p) as u8;
        }
    });
    let mut labels = majority(&labels, w, h);
    merge_small(&mut labels, w, h, cfg.speckle);
    let palette = mean_colors(&pic.rgb, &labels, centres.len(), None);
    let comps = components(&labels, w, h, false);
    let jobs: Vec<usize> = (0..comps.area.len()).collect();
    let traced = par_map(jobs, |c| {
        let ring = trace(&comps.map, w, h, c as u32, comps.first[c]);
        let enclosed = area(&ring).abs();
        let segs = fit_ring(&ring, [0.0, 0.0, w as f32, h as f32], cfg);
        let poly = flatten(&segs, scale);
        (enclosed, Shape { color: comps.value[c] as usize, rings: vec![segs], polys: vec![poly] })
    });
    let mut traced = traced;
    traced.sort_by(|a, b| b.0.total_cmp(&a.0));
    let ground = traced.first().map(|t| t.1.color);
    Vector { w, h, palette, ground, shapes: traced.into_iter().map(|t| t.1).collect(), strokes: vec![] }
}

/// The ink of a drawing, as delyrism's segmentation finds it: paper flattened, ink scored by
/// chroma and darkness with hysteresis, specks and border shapes dropped, pin-holes closed, and
/// each ink pixel labelled with a palette colour (`PAPER` elsewhere).
struct Ink {
    labels: Vec<u8>,
    ink: Vec<bool>,
    palette: Vec<[f32; 3]>,
    pen: f32,
    /// The box worked on, in the working frame (x0, y0, x1, y1, exclusive); the maps above cover it.
    at: [usize; 4],
}

impl Ink {
    fn size(&self) -> (usize, usize) {
        (self.at[2] - self.at[0], self.at[3] - self.at[1])
    }

    /// Where the frame's edges lie in this box's coordinates.
    fn edges(&self, w: usize, h: usize) -> [f32; 4] {
        let (x0, y0) = (self.at[0] as f32, self.at[1] as f32);
        [-x0, -y0, w as f32 - x0, h as f32 - y0]
    }
}

fn crop<T: Copy>(src: &[T], w: usize, at: [usize; 4]) -> Vec<T> {
    (at[1]..at[3]).flat_map(|y| src[y * w + at[0]..y * w + at[2]].iter().copied()).collect()
}

/// Moves shapes and strokes traced in a box to the frame.
fn shift(shapes: &mut [Shape], strokes: &mut [Stroke], by: P) {
    for s in shapes {
        s.rings.iter_mut().flatten().flatten().for_each(|p| *p = add(*p, by));
        s.polys.iter_mut().flatten().for_each(|p| *p = add(*p, by));
    }
    for st in strokes {
        st.points.iter_mut().for_each(|p| *p = add(*p, by));
    }
}

/// Divide by the paper's own colour: the upper quartile over a 32 px copy, drawn back up
/// bilinearly. Paper is the bright part of any window, so ink has to fill three quarters of a
/// window before it is taken for paper; delyrism's median gives way at half.
fn flatten_paper(pic: &Picture) -> Vec<[f32; 3]> {
    let (w, h) = (pic.w, pic.h);
    let f = (w.max(h) as f32 / 32.0).max(1.0);
    let (sw, sh) = (((w as f32 / f).round() as usize).max(1), ((h as f32 / f).round() as usize).max(1));
    let mut small = vec![[0f32; 3]; sw * sh];
    for sy in 0..sh {
        for sx in 0..sw {
            let (x0, x1) = (sx * w / sw, ((sx + 1) * w / sw).max(sx * w / sw + 1));
            let (y0, y1) = (sy * h / sh, ((sy + 1) * h / sh).max(sy * h / sh + 1));
            let mut acc = [0f32; 3];
            for y in y0..y1 {
                for px in &pic.rgb[y * w + x0..y * w + x1] {
                    acc = [acc[0] + px[0], acc[1] + px[1], acc[2] + px[2]];
                }
            }
            let n = ((x1 - x0) * (y1 - y0)) as f32;
            small[sy * sw + sx] = acc.map(|v| v / n);
        }
    }
    // A window a sixth of the frame wide, as delyrism's.
    let r = sw.max(sh) / 12;
    let mut bg = vec![[1f32; 3]; sw * sh];
    let mut win: Vec<f32> = Vec::new();
    for sy in 0..sh {
        for sx in 0..sw {
            for d in 0..3 {
                win.clear();
                for y in sy.saturating_sub(r)..(sy + r + 1).min(sh) {
                    for x in sx.saturating_sub(r)..(sx + r + 1).min(sw) {
                        win.push(small[y * sw + x][d]);
                    }
                }
                let at = (win.len() - 1) * 3 / 4;
                bg[sy * sw + sx][d] = win.select_nth_unstable_by(at, f32::total_cmp).1.max(1e-3);
            }
        }
    }
    let at = |v: f32, n: usize| {
        let s = v.max(0.0).min((n - 1) as f32);
        let i = (s as usize).min(n.saturating_sub(2));
        (i, (i + 1).min(n - 1), (s - i as f32).clamp(0.0, 1.0))
    };
    let cols: Vec<(usize, usize, f32)> = (0..w).map(|x| at((x as f32 + 0.5) * sw as f32 / w as f32 - 0.5, sw)).collect();
    let mut out = vec![[0f32; 3]; w * h];
    par_fill(&mut out, w, |row0, part| {
        for (k, o) in part.iter_mut().enumerate() {
            let (x, y) = (k % w, row0 + k / w);
            let (iy, iy1, fy) = at((y as f32 + 0.5) * sh as f32 / h as f32 - 0.5, sh);
            let (ix, ix1, fx) = cols[x];
            let px = pic.rgb[y * w + x];
            *o = [0, 1, 2].map(|d| {
                let top = bg[iy * sw + ix][d] * (1.0 - fx) + bg[iy * sw + ix1][d] * fx;
                let bot = bg[iy1 * sw + ix][d] * (1.0 - fx) + bg[iy1 * sw + ix1][d] * fx;
                (px[d] / (top * (1.0 - fy) + bot * fy)).clamp(0.0, 1.0)
            });
        }
    });
    out
}

fn ink(pic: &Picture, cfg: &Config, warm: &mut Vec<Lab>) -> Option<Ink> {
    let (w, h) = (pic.w, pic.h);
    let flat = flatten_paper(pic);
    let lab = to_lab(&flat, w);
    let (chi, clo, dhi, dlo) = (cfg.chroma, cfg.chroma * 0.48, cfg.dark, cfg.dark * 0.55);
    let score = |p: &Lab| ((100.0 - p[0]) / 100.0, p[1].hypot(p[2]) / 100.0);
    let hi: Vec<bool> = lab.iter().map(|p| { let (d, c) = score(p); c > chi || d > dhi }).collect();
    let lo: Vec<u8> = lab.iter().map(|p| { let (d, c) = score(p); (c > clo || d > dlo) as u8 }).collect();
    let hi = morph(&morph(&hi, w, h, SQUARE, true), w, h, SQUARE, false);
    // Hysteresis: faint ink is kept where it touches confident ink.
    let lc = components(&lo, w, h, true);
    let mut keep = vec![false; lc.area.len()];
    for i in 0..w * h {
        if hi[i] && lo[i] == 1 {
            keep[lc.map[i] as usize] = true;
        }
    }
    let ink: Vec<bool> = (0..w * h).map(|i| lo[i] == 1 && keep[lc.map[i] as usize]).collect();
    let ink = morph(&morph(&ink, w, h, CROSS, false), w, h, CROSS, true);
    if !ink.iter().any(|&v| v) {
        return None;
    }
    // From here on only the ink's box and a margin of paper are worked on.
    let (fw, fh) = (w, h);
    let at = around(&ink, w, h);
    let at = [at[0].saturating_sub(3), at[1].saturating_sub(3), (at[2] + 3).min(fw), (at[3] + 3).min(fh)];
    let (w, h) = (at[2] - at[0], at[3] - at[1]);
    let (flat, lab, mut ink) = (crop(&flat, fw, at), crop(&lab, fw, at), crop(&ink, fw, at));
    let total = ink.iter().filter(|&&v| v).count();
    let pen = pen_width(&ink, w, h);
    // Shapes touching the frame (the sheet's edge, a table's shadow) and specks smaller than a dot.
    let v: Vec<u8> = ink.iter().map(|&s| s as u8).collect();
    let ic = components(&v, w, h, true);
    let drop: Vec<bool> = (0..ic.area.len())
        .map(|c| {
            let b = ic.bbox[c];
            let (bx0, by0) = (b[0] as usize + at[0], b[1] as usize + at[1]);
            let (bx1, by1) = (b[2] as usize + at[0], b[3] as usize + at[1]);
            let touches = bx0 <= 1 || by0 <= 1 || bx1 + 2 >= fw || by1 + 2 >= fh;
            ic.value[c] == 1 && ((touches && (ic.area[c] as usize) * 2 < total) || (ic.area[c] as f32) < 1.5 * pen * pen)
        })
        .collect();
    for (s, &c) in ink.iter_mut().zip(&ic.map) {
        if drop[c as usize] {
            *s = false;
        }
    }
    // Pin-holes smaller than a pen dot: marker streaks inside a line.
    let v: Vec<u8> = ink.iter().map(|&s| s as u8).collect();
    let hc = components(&v, w, h, false);
    for (s, &c) in ink.iter_mut().zip(&hc.map) {
        if hc.value[c as usize] == 0 && (hc.area[c as usize] as f32) < 0.6 * pen * pen {
            *s = true;
        }
    }
    if !ink.iter().any(|&v| v) {
        return None;
    }

    // Palette: k-means on confident ink, near colours and stray paper merged.
    let mut core = morph(&ink, w, h, SQUARE, true);
    if core.iter().filter(|&&v| v).count() < 50 {
        core.clone_from(&ink);
    }
    let xs: Vec<Lab> = (0..w * h).filter(|&i| core[i]).map(|i| lab[i]).collect();
    let xs = sample(&xs, 8000);
    let k = cfg.colors.min((xs.len() / 50).max(1));
    let fresh = warm.is_empty();
    let mut c = kmeans(&xs, k, warm, if fresh { 16 } else { 6 });
    let mut cnt = vec![0f64; c.len()];
    for &p in &xs {
        cnt[nearest(&c, p)] += 1.0;
    }
    while c.len() > 1 {
        let n = c.len();
        let sum: f64 = cnt.iter().sum();
        let d = |i: usize, j: usize| if i == j { f32::INFINITY } else { ink_dist(c[i], c[j]) };
        let closest = |i: usize| (0..n).min_by(|&a, &b| d(i, a).total_cmp(&d(i, b))).unwrap_or(0);
        if let Some(q) = (0..n).find(|&q| c[q][0] > 90.0 && c[q][1].hypot(c[q][2]) < 10.0) {
            let j = closest(q);
            cnt[j] += cnt[q];
            c.remove(q);
            cnt.remove(q);
            continue;
        }
        let (mut i, mut j, mut best) = (0, 0, f32::INFINITY);
        for a in 0..n {
            for b in 0..n {
                if d(a, b) < best {
                    (i, j, best) = (a, b, d(a, b));
                }
            }
        }
        if best >= cfg.merge {
            let tiny = (0..n).min_by(|&a, &b| cnt[a].total_cmp(&cnt[b])).unwrap_or(0);
            if cnt[tiny] / sum >= 0.01 {
                break;
            }
            (i, j) = (tiny, closest(tiny));
        }
        let wsum = cnt[i] + cnt[j];
        c[i] = [0, 1, 2].map(|k| ((c[i][k] as f64 * cnt[i] + c[j][k] as f64 * cnt[j]) / wsum) as f32);
        cnt[i] = wsum;
        c.remove(j);
        cnt.remove(j);
    }
    warm.clone_from(&c);

    // Confident ink takes its nearest colour, hue first; edge pixels, mixed with paper and shifted
    // by shadow, take the colour of the nearest confident pixel.
    let mut inner = morph(&morph(&ink, w, h, SQUARE, true), w, h, CROSS, true);
    if inner.iter().filter(|&&v| v).count() * 5 < total {
        inner.clone_from(&core);
    }
    let mut labels: Vec<u8> = (0..w * h)
        .map(|i| {
            if !inner[i] {
                return PAPER;
            }
            let mut best = (f32::MAX, 0u8);
            for (j, &q) in c.iter().enumerate() {
                let e = ink_dist(lab[i], q);
                if e < best.0 {
                    best = (e, j as u8);
                }
            }
            best.1
        })
        .collect();
    if labels.iter().all(|&l| l == PAPER) {
        labels.iter_mut().zip(&ink).for_each(|(l, &s)| if s { *l = 0 });
    }
    nearest_seed(&mut labels, &ink, w, h);
    labels.iter_mut().zip(&ink).for_each(|(l, &s)| if !s { *l = PAPER });
    merge_small(&mut labels, w, h, 1.2 * pen * pen);

    // A colour holding too little ink, or paper-white, goes to the nearest pixel of another.
    let n = c.len();
    let mut used = vec![0usize; n];
    let mut light = vec![[0f64; 3]; n];
    for i in 0..w * h {
        let l = labels[i] as usize;
        if l < n {
            used[l] += 1;
            for d in 0..3 {
                light[l][d] += lab[i][d] as f64;
            }
        }
    }
    let inked: usize = used.iter().sum();
    let largest = (0..n).max_by_key(|&j| used[j]).unwrap_or(0);
    let bad: Vec<bool> = (0..n)
        .map(|j| {
            if used[j] == 0 || j == largest {
                return false;
            }
            let m = light[j].map(|v| (v / used[j] as f64) as f32);
            (used[j] as f64) < 0.01 * inked as f64 || (m[0] > 90.0 && m[1].hypot(m[2]) < 10.0)
        })
        .collect();
    if bad.iter().any(|&b| b) {
        labels.iter_mut().for_each(|l| if (*l as usize) < n && bad[*l as usize] { *l = PAPER });
        nearest_seed(&mut labels, &ink, w, h);
        labels.iter_mut().zip(&ink).for_each(|(l, &s)| if !s { *l = PAPER });
        used = vec![0; n];
        labels.iter().for_each(|&l| if (l as usize) < n { used[l as usize] += 1 });
    }
    // Ordered by share of ink; each colour is the mean of its own confident pixels.
    let mut order: Vec<usize> = (0..n).filter(|&j| used[j] > 0).collect();
    order.sort_by_key(|&j| std::cmp::Reverse(used[j]));
    let mut remap = [PAPER; 256];
    for (to, &from) in order.iter().enumerate() {
        remap[from] = to as u8;
    }
    labels.iter_mut().for_each(|l| *l = remap[*l as usize]);
    let mut palette = mean_colors(&flat, &labels, order.len(), Some(&core));
    let loose = mean_colors(&flat, &labels, order.len(), None);
    for j in 0..order.len() {
        if !(0..w * h).any(|i| labels[i] as usize == j && core[i]) {
            palette[j] = loose[j];
        }
    }
    Some(Ink { labels, ink, palette, pen, at })
}

/// Median stroke thickness: twice the distance to paper along the skeleton.
fn pen_width(m: &[bool], w: usize, h: usize) -> f32 {
    let dt = distance(m, w, h);
    let sk = thin(m, w, h);
    let mut v: Vec<f32> = (0..w * h).filter(|&i| sk[i]).map(|i| dt[i]).collect();
    if v.len() < 10 {
        return 1.0;
    }
    (2.0 * median(&mut v)).max(1.0)
}

/// A mask's outline: an outer ring for each 4-connected piece, and a reversed ring for each hole
/// (a piece of ground that the 8-connected ground around it does not lead out of the frame).
fn outline(mask: &[bool], w: usize, h: usize, edges: [f32; 4], cfg: &Config, color: usize, scale: f32) -> Option<Shape> {
    let v: Vec<u8> = mask.iter().map(|&s| s as u8).collect();
    let pieces = components(&v, w, h, false);
    let ground = components(&v, w, h, true);
    let edge = |b: [u32; 4]| b[0] == 0 || b[1] == 0 || b[2] as usize + 1 == w || b[3] as usize + 1 == h;
    let jobs: Vec<usize> = (0..pieces.area.len())
        .filter(|&c| pieces.value[c] == 1 || (pieces.value[c] == 0 && !edge(ground.bbox[ground.map[pieces.first[c] as usize] as usize])))
        .collect();
    let rings: Vec<Option<Vec<Seg>>> = par_map(jobs, |c| {
        let mut ring = trace(&pieces.map, w, h, c as u32, pieces.first[c]);
        if area(&ring).abs() < 4.0 {
            return None;
        }
        if pieces.value[c] == 0 {
            ring.reverse();
        }
        Some(fit_ring(&ring, edges, cfg))
    });
    let rings: Vec<Vec<Seg>> = rings.into_iter().flatten().collect();
    if rings.is_empty() {
        return None;
    }
    let polys = rings.iter().map(|r| flatten(r, scale)).collect();
    Some(Shape { color, rings, polys })
}

/// `ink`: each ink colour outlined. The dominant colour fills the whole silhouette and the others
/// sit on top, so neighbouring colours meet without seams.
fn ink_shapes(pic: &Picture, cfg: &Config, warm: &mut Vec<Lab>, scale: f32) -> Vector {
    let (w, h) = (pic.w, pic.h);
    let Some(ink) = ink(pic, cfg, warm) else { return Vector { w, h, palette: vec![], ground: None, shapes: vec![], strokes: vec![] } };
    let ((bw, bh), edges) = (ink.size(), ink.edges(w, h));
    let mut shapes = Vec::new();
    for c in 0..ink.palette.len() {
        let mask: Vec<bool> = if c == 0 { ink.ink.clone() } else { ink.labels.iter().map(|&l| l as usize == c).collect() };
        shapes.extend(outline(&mask, bw, bh, edges, cfg, c, scale));
    }
    shift(&mut shapes, &mut [], [ink.at[0] as f32, ink.at[1] as f32]);
    Vector { w, h, palette: ink.palette, ground: None, shapes, strokes: vec![] }
}

// ------------------------------------------------------------------------------------------------
//  Centre lines
// ------------------------------------------------------------------------------------------------

const DIRS: [(i32, i32); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

/// The skeleton's edges as a bitmask per pixel: 4-neighbours always, a diagonal only where no
/// 4-neighbour already joins the two (a staircase is a line, not a branch).
fn adjacency(sk: &[bool], w: usize, h: usize) -> Vec<u8> {
    let on = |x: i32, y: i32| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && sk[y as usize * w + x as usize];
    let mut adj = vec![0u8; w * h];
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            if !on(x, y) {
                continue;
            }
            for (d, &(dx, dy)) in DIRS.iter().enumerate() {
                if !on(x + dx, y + dy) {
                    continue;
                }
                if dx != 0 && dy != 0 && (on(x + dx, y) || on(x, y + dy)) {
                    continue;
                }
                adj[y as usize * w + x as usize] |= 1 << d;
            }
        }
    }
    adj
}

fn step(i: usize, d: usize, w: usize) -> usize {
    (i as i64 + DIRS[d].0 as i64 + DIRS[d].1 as i64 * w as i64) as usize
}

/// Pixel chains between branch points and ends, plus rings with no branch point.
fn chains(adj: &[u8], w: usize) -> Vec<Vec<u32>> {
    let node = |i: usize| adj[i] != 0 && adj[i].count_ones() != 2;
    let mut seen = vec![0u8; adj.len()];
    let mut out = Vec::new();
    let walk = |start: usize, d0: usize, seen: &mut Vec<u8>, ring: bool| -> Vec<u32> {
        let mut path = vec![start as u32];
        let (mut cur, mut d) = (start, d0);
        loop {
            let q = step(cur, d, w);
            seen[cur] |= 1 << d;
            seen[q] |= 1 << ((d + 4) % 8);
            path.push(q as u32);
            if (ring && q == start) || (!ring && node(q)) {
                break;
            }
            let Some(nd) = (0..8).find(|&k| adj[q] & (1 << k) != 0 && seen[q] & (1 << k) == 0) else { break };
            (cur, d) = (q, nd);
        }
        path
    };
    for i in 0..adj.len() {
        if !node(i) {
            continue;
        }
        for d in 0..8 {
            if adj[i] & (1 << d) != 0 && seen[i] & (1 << d) == 0 {
                out.push(walk(i, d, &mut seen, false));
            }
        }
    }
    for i in 0..adj.len() {
        if adj[i] != 0 && seen[i] == 0 {
            let d = (0..8).find(|&k| adj[i] & (1 << k) != 0).expect("a ring pixel has an edge");
            let path = walk(i, d, &mut seen, true);
            if path.len() >= 8 {
                out.push(path);
            }
        }
    }
    out
}

/// Short end branches left by thinning go, repeatedly, since pruning one exposes the next.
fn prune(mut chains: Vec<Vec<u32>>, spur: f32, n: usize) -> Vec<Vec<u32>> {
    let mut deg = vec![0u16; n];
    for _ in 0..4 {
        for c in &chains {
            deg[c[0] as usize] += 1;
            deg[c[c.len() - 1] as usize] += 1;
        }
        let before = chains.len();
        let kept: Vec<Vec<u32>> = chains
            .iter()
            .filter(|c| !((c.len() as f32) < spur && (deg[c[0] as usize] == 1) != (deg[c[c.len() - 1] as usize] == 1)))
            .cloned()
            .collect();
        for c in &chains {
            deg[c[0] as usize] = 0;
            deg[c[c.len() - 1] as usize] = 0;
        }
        chains = kept;
        if chains.len() == before {
            break;
        }
    }
    chains
}

fn xy(i: u32, w: usize) -> P {
    [(i as usize % w) as f32, (i as usize / w) as f32]
}

/// The direction a chain leaves one end in, read between one and three pen widths from the
/// junction, where thinning has stopped bending it.
fn leaving(c: &[u32], at_start: bool, pen: f32, w: usize) -> P {
    let n = c.len();
    let at = |k: usize| if at_start { xy(c[k], w) } else { xy(c[n - 1 - k], w) };
    let (mut i0, mut i1) = ((pen as usize).min(n / 3), ((3.0 * pen) as usize).min(n - 1));
    if i1 <= i0 {
        (i0, i1) = (0, n - 1);
    }
    unit(sub(at(i1), at(i0)))
}

/// Chains that run on smoothly through a junction become one stroke, so a "+" is two strokes.
/// Thinning splits a crossing of two thick lines into two branch points about a pen width apart,
/// joined by a short bridge; branch points that close are one junction, and the bridge is dropped.
fn join(chains: &[Vec<u32>], pen: f32, w: usize, n: usize) -> Vec<Vec<P>> {
    let tol = (1.1 * pen).max(3.0);
    let mut ends: Vec<(usize, usize, P)> = Vec::new();
    for (i, c) in chains.iter().enumerate() {
        ends.push((i, 0, xy(c[0], w)));
        if c[0] != c[c.len() - 1] {
            ends.push((i, 1, xy(c[c.len() - 1], w)));
        }
    }
    let mut parent: Vec<usize> = (0..ends.len()).collect();
    fn find(p: &mut [usize], mut a: usize) -> usize {
        while p[a] != a {
            p[a] = p[p[a]];
            a = p[a];
        }
        a
    }
    let unite = |p: &mut Vec<usize>, a: usize, b: usize| {
        let (ra, rb) = (find(p, a), find(p, b));
        if ra != rb {
            p[ra] = rb;
        }
    };
    for a in 0..ends.len() {
        for b in a + 1..ends.len() {
            let d = sub(ends[a].2, ends[b].2);
            if d[0].abs().max(d[1].abs()) <= 2.0 {
                unite(&mut parent, a, b);
            }
        }
    }
    let mut deg = vec![0u16; n];
    for c in chains {
        deg[c[0] as usize] += 1;
        deg[c[c.len() - 1] as usize] += 1;
    }
    let mut end_of = vec![[usize::MAX; 2]; chains.len()];
    for (a, &(i, e, _)) in ends.iter().enumerate() {
        end_of[i][e] = a;
    }
    let mut bridge = vec![false; chains.len()];
    for (i, c) in chains.iter().enumerate() {
        let (s, t) = (c[0] as usize, c[c.len() - 1] as usize);
        if s != t && (c.len() as f32) <= tol && deg[s] >= 2 && deg[t] >= 2 {
            bridge[i] = true;
            unite(&mut parent, end_of[i][0], end_of[i][1]);
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for a in 0..ends.len() {
        if !bridge[ends[a].0] {
            groups.entry(find(&mut parent, a)).or_default().push(a);
        }
    }
    let mut link = vec![[None::<(usize, usize)>; 2]; chains.len()];
    for g in groups.values() {
        if g.len() < 2 {
            continue;
        }
        let dirs: Vec<P> = g.iter().map(|&a| leaving(&chains[ends[a].0], ends[a].1 == 0, pen, w)).collect();
        let mut pairs: Vec<(f32, usize, usize)> = Vec::new();
        for x in 0..g.len() {
            for y in x + 1..g.len() {
                if ends[g[x]].0 != ends[g[y]].0 {
                    pairs.push((dot(dirs[x], dirs[y]), g[x], g[y]));
                }
            }
        }
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut free: Vec<bool> = vec![true; ends.len()];
        for (d, a, b) in pairs {
            if d > -0.7 {
                break;
            }
            if free[a] && free[b] {
                let (ea, eb) = ((ends[a].0, ends[a].1), (ends[b].0, ends[b].1));
                link[ea.0][ea.1] = Some(eb);
                link[eb.0][eb.1] = Some(ea);
                (free[a], free[b]) = (false, false);
            }
        }
    }
    let mut done = bridge.clone();
    let mut strokes: Vec<Vec<P>> = Vec::new();
    let walk = |i0: usize, e0: usize, done: &mut Vec<bool>| {
        let mut pts: Vec<P> = Vec::new();
        let (mut i, mut e) = (i0, e0);
        while !done[i] {
            done[i] = true;
            let c = &chains[i];
            let seq: Box<dyn Iterator<Item = &u32>> = if e == 0 { Box::new(c.iter()) } else { Box::new(c.iter().rev()) };
            let seq: Vec<P> = seq.map(|&q| xy(q, w)).collect();
            if let Some(&last) = pts.last() {
                let gap = sub(seq[0], last);
                let m = gap[0].abs().max(gap[1].abs()) as usize;
                // Across a collapsed junction: bridged in a straight line.
                for t in 1..m {
                    let p = add(last, mul(gap, t as f32 / m as f32));
                    pts.push([p[0].round(), p[1].round()]);
                }
                pts.extend_from_slice(&seq[1..]);
            } else {
                pts.extend_from_slice(&seq);
            }
            let Some(next) = link[i][1 - e] else { break };
            (i, e) = next;
        }
        pts
    };
    for i in 0..chains.len() {
        for e in 0..2 {
            if end_of[i][e] != usize::MAX && link[i][e].is_none() && !done[i] {
                let s = walk(i, e, &mut done);
                strokes.push(s);
            }
        }
    }
    for i in 0..chains.len() {
        if !done[i] {
            let s = walk(i, 0, &mut done);
            strokes.push(s);
        }
    }
    strokes.retain(|s| s.len() >= 2);
    strokes
}

/// `lines`: each colour's pen paths as centre lines with the pen width measured along them. A
/// region thicker than `fill` pen widths (a coloured-in shape) is outlined instead, and a blob
/// smaller than a few pen widths is a dot.
fn line_strokes(pic: &Picture, cfg: &Config, warm: &mut Vec<Lab>, scale: f32) -> Vector {
    let (w, h) = (pic.w, pic.h);
    let Some(ink) = ink(pic, cfg, warm) else { return Vector { w, h, palette: vec![], ground: None, shapes: vec![], strokes: vec![] } };
    let (fw, fh) = (w, h);
    let ((w, h), edges) = (ink.size(), ink.edges(fw, fh));
    let pen = ink.pen;
    let mut shapes = Vec::new();
    let mut strokes: Vec<Stroke> = Vec::new();
    for c in 0..ink.palette.len() {
        let m: Vec<bool> = ink.labels.iter().map(|&l| l as usize == c).collect();
        let dt = distance(&m, w, h);
        let sk = thin(&m, w, h);
        let v: Vec<u8> = m.iter().map(|&s| s as u8).collect();
        let comps = components(&v, w, h, true);
        let mut thick: Vec<Vec<f32>> = vec![Vec::new(); comps.area.len()];
        for i in 0..w * h {
            if sk[i] {
                thick[comps.map[i] as usize].push(dt[i]);
            }
        }
        // 0 ground, 1 line, 2 fill, 3 dot
        let kind: Vec<u8> = (0..comps.area.len())
            .map(|k| {
                if comps.value[k] == 0 {
                    return 0;
                }
                let b = comps.bbox[k];
                let (bw, bh) = ((b[2] - b[0] + 1) as f32, (b[3] - b[1] + 1) as f32);
                let t = &mut thick[k];
                // The 90th percentile: a coloured-in wedge is thick over much of its length, a line
                // only at its crossings.
                let p90 = if t.is_empty() {
                    0.0
                } else {
                    let at = ((t.len() - 1) as f32 * 0.9).round() as usize;
                    2.0 * *t.select_nth_unstable_by(at, f32::total_cmp).1
                };
                if bw.max(bh) < 2.5 * pen && comps.area[k] as f32 > 0.4 * bw * bh {
                    3
                } else if p90 > cfg.fill * pen {
                    2
                } else {
                    1
                }
            })
            .collect();
        for k in (0..kind.len()).filter(|&k| kind[k] == 3) {
            let (mut sx, mut sy, mut n) = (0f32, 0f32, 0f32);
            let b = comps.bbox[k];
            for y in b[1]..=b[3] {
                for x in b[0]..=b[2] {
                    if comps.map[y as usize * w + x as usize] == k as u32 {
                        (sx, sy, n) = (sx + x as f32, sy + y as f32, n + 1.0);
                    }
                }
            }
            let size = (b[2] - b[0] + 1).max(b[3] - b[1] + 1) as f32;
            strokes.push(Stroke { points: vec![[sx / n + 0.5, sy / n + 0.5]], color: c, width: size, closed: false, dot: true });
        }
        let fills: Vec<bool> = (0..w * h).map(|i| kind[comps.map[i] as usize] == 2).collect();
        if fills.iter().any(|&f| f) {
            shapes.extend(outline(&fills, w, h, edges, cfg, c, scale));
        }
        let line_sk: Vec<bool> = (0..w * h).map(|i| sk[i] && kind[comps.map[i] as usize] == 1).collect();
        let adj = adjacency(&line_sk, w, h);
        let ch = prune(chains(&adj, w), 1.2 * pen, w * h);
        for s in join(&ch, pen, w, w * h) {
            let p: Vec<P> = s.iter().map(|q| [q[0] + 0.5, q[1] + 0.5]).collect();
            let closed = p.len() > 8 && norm(sub(p[0], p[p.len() - 1])) < 3.0;
            let mut q = resample(&p, 1.0);
            let sigma = (0.35 * pen).max(1.0);
            if q.len() > 4 {
                q = if closed {
                    let mut r = smooth_closed(&q[..q.len() - 1], sigma);
                    r.push(r[0]);
                    r
                } else {
                    smooth_open(&q, sigma)
                };
            }
            let (lo, hi) = q.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), p| ([lo[0].min(p[0]), lo[1].min(p[1])], [hi[0].max(p[0]), hi[1].max(p[1])]));
            if norm(sub(hi, lo)) < 0.8 * pen {
                continue;
            }
            let mut widths: Vec<f32> = s
                .iter()
                .map(|q| dt[(q[1] as usize).min(h - 1) * w + (q[0] as usize).min(w - 1)])
                .collect();
            let width = (2.0 * median(&mut widths)).max(1.0);
            strokes.push(Stroke { points: q, color: c, width, closed, dot: false });
        }
    }
    shift(&mut shapes, &mut strokes, [ink.at[0] as f32, ink.at[1] as f32]);
    order(&mut strokes, fw.max(fh) as f32);
    Vector { w: fw, h: fh, palette: ink.palette, ground: None, shapes, strokes }
}

/// Top to bottom, then left to right; a stroke starts at its upper-left end, a loop at its top
/// point running counter-clockwise on the page.
fn order(strokes: &mut [Stroke], side: f32) {
    for s in strokes.iter_mut() {
        if s.dot {
            continue;
        }
        let p = &mut s.points;
        if s.closed && p.len() > 2 {
            let body = &p[..p.len() - 1];
            let k = (0..body.len()).min_by(|&a, &b| body[a][1].total_cmp(&body[b][1])).unwrap_or(0);
            let mut q: Vec<P> = (0..body.len()).map(|i| body[(i + k) % body.len()]).collect();
            q.push(q[0]);
            if area(&q[..q.len() - 1]) > 0.0 {
                q.reverse();
            }
            *p = q;
        } else if p[p.len() - 1][1] + 0.5 * p[p.len() - 1][0] < p[0][1] + 0.5 * p[0][0] {
            p.reverse();
        }
    }
    let band = (0.02 * side).max(1.0);
    strokes.sort_by(|a, b| {
        let (pa, pb) = (a.points[0], b.points[0]);
        ((pa[1] / band).round(), pa[0]).partial_cmp(&((pb[1] / band).round(), pb[0])).unwrap_or(std::cmp::Ordering::Equal)
    });
}

// ------------------------------------------------------------------------------------------------
//  Drawing the curves back, and writing them out
// ------------------------------------------------------------------------------------------------

/// Exact-area polygon coverage over the rows `y0..y1`, accumulated per edge and summed along
/// each row (font-rs).
struct Raster {
    w: usize,
    y0: usize,
    y1: usize,
    acc: Vec<f32>,
    box_: [usize; 4],
}

impl Raster {
    fn new(w: usize, y0: usize, y1: usize) -> Raster {
        Raster { w, y0, y1, acc: vec![0.0; (w + 2) * (y1 - y0)], box_: [usize::MAX, usize::MAX, 0, 0] }
    }

    /// One edge; the part left of the frame counts at its left edge, the part right of it nowhere.
    fn line(&mut self, a: P, b: P) {
        if a[1].max(b[1]) <= self.y0 as f32 || a[1].min(b[1]) >= self.y1 as f32 {
            return;
        }
        let fw = self.w as f32;
        let mut ts = [0.0f32, 1.0, 1.0, 1.0];
        let mut n = 1;
        for edge in [0.0, fw] {
            if (a[0] < edge) != (b[0] < edge) {
                ts[n] = (edge - a[0]) / (b[0] - a[0]);
                n += 1;
            }
        }
        ts[n] = 1.0;
        ts[..=n].sort_by(f32::total_cmp);
        for k in 0..n {
            let at = |t: f32| {
                let p = add(a, mul(sub(b, a), t));
                [p[0].clamp(0.0, fw), p[1]]
            };
            self.edge(at(ts[k]), at(ts[k + 1]));
        }
    }

    fn edge(&mut self, p0: P, p1: P) {
        if (p0[1] - p1[1]).abs() <= 1e-6 {
            return;
        }
        let (dir, p0, p1) = if p0[1] < p1[1] { (1.0, p0, p1) } else { (-1.0, p1, p0) };
        let (top, bottom) = (self.y0 as f32, self.y1 as f32);
        if p1[1] <= top || p0[1] >= bottom {
            return;
        }
        let stride = self.w + 2;
        let dxdy = (p1[0] - p0[0]) / (p1[1] - p0[1]);
        let start = p0[1].max(top);
        let mut x = p0[0] + (start - p0[1]) * dxdy;
        let ya = start as usize;
        let yb = (p1[1].ceil() as usize).min(self.y1);
        let b = &mut self.box_;
        b[1] = b[1].min(ya);
        b[3] = b[3].max(yb);
        b[0] = b[0].min(p0[0].min(p1[0]).max(0.0) as usize);
        b[2] = b[2].max((p0[0].max(p1[0]).ceil() as usize + 2).min(stride));
        for y in ya..yb {
            let row = (y - self.y0) * stride;
            let dy = ((y + 1) as f32).min(p1[1]) - (y as f32).max(p0[1]);
            let xnext = x + dxdy * dy;
            let d = dy * dir;
            let (x0, x1) = if x < xnext { (x, xnext) } else { (xnext, x) };
            let x0f = x0.floor();
            let x0i = x0f as usize;
            let x1c = x1.ceil();
            let x1i = x1c as usize;
            let a = &mut self.acc;
            if x1i <= x0i + 1 {
                let xmf = 0.5 * (x + xnext) - x0f;
                a[row + x0i] += d - d * xmf;
                a[row + x0i + 1] += d * xmf;
            } else {
                let s = 1.0 / (x1 - x0);
                let x0r = x0 - x0f;
                let a0 = 0.5 * s * (1.0 - x0r) * (1.0 - x0r);
                let x1r = x1 - x1c + 1.0;
                let am = 0.5 * s * x1r * x1r;
                a[row + x0i] += d * a0;
                if x1i == x0i + 2 {
                    a[row + x0i + 1] += d * (1.0 - a0 - am);
                } else {
                    let a1 = s * (1.5 - x0r);
                    a[row + x0i + 1] += d * (a1 - a0);
                    for xi in x0i + 2..x1i - 1 {
                        a[row + xi] += d * s;
                    }
                    let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                    a[row + x1i - 1] += d * (1.0 - a2 - am);
                }
                a[row + x1i] += d * am;
            }
            x = xnext;
        }
    }

    /// Lays the accumulated shape over `img` (this band's rows) in `rgb`, nonzero winding, then clears.
    fn paint(&mut self, img: &mut [f32], rgb: [f32; 3]) {
        let [x0, y0, x1, y1] = self.box_;
        if x0 == usize::MAX {
            return;
        }
        let stride = self.w + 2;
        for y in y0..y1 {
            let (row, px) = ((y - self.y0) * stride, (y - self.y0) * self.w);
            let mut s = 0.0;
            for x in x0..x1 {
                s += self.acc[row + x];
                self.acc[row + x] = 0.0;
                if x < self.w {
                    over(img, px + x, rgb, s.abs().min(1.0));
                }
            }
        }
        self.box_ = [usize::MAX, usize::MAX, 0, 0];
    }
}

/// Straight-alpha "over" of an opaque colour at coverage `a`.
fn over(img: &mut [f32], px: usize, rgb: [f32; 3], a: f32) {
    if a <= 1.0 / 1024.0 {
        return;
    }
    let o = px * 4;
    let da = img[o + 3];
    let keep = da * (1.0 - a);
    let oa = a + keep;
    for k in 0..3 {
        img[o + k] = (rgb[k] * a + img[o + k] * keep) / oa;
    }
    img[o + 3] = oa;
}

/// Stroke coverage over the rows `row0..row0 + rows`: each pixel keeps the most coverage any
/// round-capped segment gives it, so overlapping segments of one colour make one union.
struct Coverage {
    cov: Vec<f32>,
    w: usize,
    row0: usize,
    rows: usize,
    box_: [usize; 4],
}

impl Coverage {
    fn new(w: usize, row0: usize, rows: usize) -> Coverage {
        Coverage { cov: vec![0.0; w * rows], w, row0, rows, box_: [usize::MAX, usize::MAX, 0, 0] }
    }

    fn capsule(&mut self, a: P, b: P, r: f32) {
        let top = self.row0 as f32;
        let lo = [a[0].min(b[0]) - r - 1.0, a[1].min(b[1]) - r - 1.0 - top];
        let hi = [a[0].max(b[0]) + r + 1.0, a[1].max(b[1]) + r + 1.0 - top];
        let (x0, y0) = (lo[0].max(0.0) as usize, lo[1].max(0.0) as usize);
        let (x1, y1) = ((hi[0].max(0.0) as usize).min(self.w), (hi[1].max(0.0) as usize).min(self.rows));
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let bx = &mut self.box_;
        *bx = [bx[0].min(x0), bx[1].min(y0), bx[2].max(x1), bx[3].max(y1)];
        let ab = sub(b, a);
        let l2 = dot(ab, ab);
        for y in y0..y1 {
            for x in x0..x1 {
                let ap = sub([x as f32 + 0.5, y as f32 + top + 0.5], a);
                let t = if l2 > 0.0 { (dot(ap, ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
                let c = (r + 0.5 - norm(sub(ap, mul(ab, t)))).clamp(0.0, 1.0);
                let i = y * self.w + x;
                if c > self.cov[i] {
                    self.cov[i] = c;
                }
            }
        }
    }

    /// Lays the coverage over `img` (this band's rows) in `rgb`, then clears.
    fn paint(&mut self, img: &mut [f32], rgb: [f32; 3]) {
        let [x0, y0, x1, y1] = self.box_;
        if x0 == usize::MAX {
            return;
        }
        for y in y0..y1 {
            for x in x0..x1 {
                let i = y * self.w + x;
                over(img, i, rgb, self.cov[i]);
                self.cov[i] = 0.0;
            }
        }
        self.box_ = [usize::MAX, usize::MAX, 0, 0];
    }
}

/// The vector drawn into an RGBA frame of `ow` × `oh`.
pub fn render(v: &Vector, ow: usize, oh: usize, weight: f32) -> Vec<f32> {
    let mut img = vec![0f32; ow * oh * 4];
    let (sx, sy) = (ow as f32 / v.w as f32, oh as f32 / v.h as f32);
    if let Some(g) = v.ground {
        let c = v.palette[g];
        img.chunks_exact_mut(4).for_each(|p| p.copy_from_slice(&[c[0], c[1], c[2], 1.0]));
    }
    // Row bands, one per core, each drawing every shape that reaches it, in stacking order.
    let lows: Vec<f32> = v.shapes.iter().map(|s| s.polys.iter().flatten().fold(f32::MAX, |m, p| m.min(p[1])) * sy).collect();
    let highs: Vec<f32> = v.shapes.iter().map(|s| s.polys.iter().flatten().fold(f32::MIN, |m, p| m.max(p[1])) * sy).collect();
    par_fill(&mut img, ow * 4, |row0, band| {
        let rows = band.len() / (ow * 4);
        let mut ras = Raster::new(ow, row0, row0 + rows);
        for (k, s) in v.shapes.iter().enumerate() {
            if highs[k] <= row0 as f32 || lows[k] >= (row0 + rows) as f32 {
                continue;
            }
            for poly in &s.polys {
                for i in 0..poly.len() {
                    let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
                    ras.line([a[0] * sx, a[1] * sy], [b[0] * sx, b[1] * sy]);
                }
            }
            ras.paint(band, v.palette[s.color]);
        }
    });
    if v.strokes.is_empty() {
        return img;
    }
    // Each stroke at the output scale, spaced at half its radius: closer adds nothing a capsule
    // does not already cover.
    let s = 0.5 * (sx + sy);
    let drawn: Vec<(usize, f32, Vec<P>, f32, f32)> = v
        .strokes
        .iter()
        .map(|st| {
            let r = (0.5 * st.width * s * if st.dot { 1.0 } else { weight }).max(0.5);
            let pts: Vec<P> = st.points.iter().map(|p| [p[0] * sx, p[1] * sy]).collect();
            let pts = if pts.len() > 2 { resample(&pts, (0.5 * r).max(1.0)) } else { pts };
            let (lo, hi) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])));
            (st.color, r, pts, lo - r - 1.0, hi + r + 1.0)
        })
        .collect();
    par_fill(&mut img, ow * 4, |row0, band| {
        let rows = band.len() / (ow * 4);
        let mut cov = Coverage::new(ow, row0, rows);
        for c in 0..v.palette.len() {
            for (_, r, pts, lo, hi) in drawn.iter().filter(|d| d.0 == c) {
                if *hi <= row0 as f32 || *lo >= (row0 + rows) as f32 {
                    continue;
                }
                if pts.len() == 1 {
                    cov.capsule(pts[0], pts[0], *r);
                }
                for k in 1..pts.len() {
                    cov.capsule(pts[k - 1], pts[k], *r);
                }
            }
            cov.paint(band, v.palette[c]);
        }
    });
    img
}

/// One decimal, no trailing zero: what a path needs, written without the formatter.
fn num(out: &mut String, v: f32) {
    let t = (v * 10.0).round() as i64;
    if t < 0 {
        out.push('-');
    }
    let t = t.unsigned_abs();
    let (int, frac) = (t / 10, t % 10);
    let mut buf = [0u8; 20];
    let mut n = 0;
    let mut i = int;
    loop {
        buf[n] = b'0' + (i % 10) as u8;
        n += 1;
        i /= 10;
        if i == 0 {
            break;
        }
    }
    for k in (0..n).rev() {
        out.push(buf[k] as char);
    }
    if frac != 0 {
        out.push('.');
        out.push((b'0' + frac as u8) as char);
    }
}

fn pt(out: &mut String, p: P) {
    num(out, p[0]);
    out.push(' ');
    num(out, p[1]);
}

fn path_d(out: &mut String, segs: &[Seg], closed: bool) {
    let Some(first) = segs.first() else { return };
    out.push('M');
    pt(out, first[0]);
    for s in segs {
        out.push('C');
        pt(out, s[1]);
        out.push(' ');
        pt(out, s[2]);
        out.push(' ');
        pt(out, s[3]);
    }
    if closed {
        out.push('Z');
    }
}

fn hex(c: [f32; 3]) -> String {
    let b = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("#{:02x}{:02x}{:02x}", b[0], b[1], b[2])
}

/// The SVG document: the working frame as its view box, drawn at the output size.
pub fn svg(v: &Vector, ow: usize, oh: usize, cfg: &Config) -> String {
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" width=\"{ow}\" height=\"{oh}\">\n",
        v.w, v.h
    );
    if let Some(g) = v.ground {
        out += &format!("<rect width=\"{}\" height=\"{}\" fill=\"{}\"/>\n", v.w, v.h, hex(v.palette[g]));
    }
    for s in &v.shapes {
        out += "<path fill=\"";
        out += &hex(v.palette[s.color]);
        out += "\" d=\"";
        for r in &s.rings {
            path_d(&mut out, r, true);
        }
        out += "\"/>\n";
    }
    if !v.strokes.is_empty() {
        out += "<g fill=\"none\" stroke-linecap=\"round\" stroke-linejoin=\"round\">\n";
        for st in &v.strokes {
            let col = hex(v.palette[st.color]);
            if st.dot {
                let p = st.points[0];
                out += &format!("<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"{:.1}\" fill=\"{col}\" stroke=\"none\"/>\n", p[0], p[1], st.width / 2.0);
                continue;
            }
            out += &format!("<path stroke=\"{col}\" stroke-width=\"{:.1}\" d=\"", st.width * cfg.weight);
            path_d(&mut out, &beziers(&st.points, st.closed, cfg.detail), st.closed);
            out += "\"/>\n";
        }
        out += "</g>\n";
    }
    out += "</svg>\n";
    out
}

/// Every drawn path as rows of x, y (0..1 of the frame, y down), path index and palette index.
pub fn paths(v: &Vector) -> Vec<f32> {
    let (fw, fh) = (v.w as f32, v.h as f32);
    let mut rows = Vec::new();
    let mut k = 0f32;
    for s in &v.shapes {
        for poly in &s.polys {
            for p in poly {
                rows.extend_from_slice(&[p[0] / fw, p[1] / fh, k, s.color as f32]);
            }
            k += 1.0;
        }
    }
    for st in &v.strokes {
        for p in &st.points {
            rows.extend_from_slice(&[p[0] / fw, p[1] / fh, k, st.color as f32]);
        }
        k += 1.0;
    }
    rows
}

pub struct Traced {
    pub vector: Vector,
    pub ow: usize,
    pub oh: usize,
}

/// Traces one frame. `warm` carries the palette from frame to frame, so colours hold still.
pub fn trace_frame(shape: &[usize], bytes: &[u8], cfg: &Config, warm: &mut Vec<Lab>) -> Result<Traced, String> {
    let (pic, iw, ih) = picture(shape, bytes, cfg.size)?;
    let (ow, oh) = if cfg.resolution == 0 {
        (iw, ih)
    } else {
        let s = cfg.resolution as f32 / iw.max(ih) as f32;
        (((iw as f32 * s).round() as usize).max(1), ((ih as f32 * s).round() as usize).max(1))
    };
    let scale = 0.5 * (ow as f32 / pic.w as f32 + oh as f32 / pic.h as f32);
    let vector = match cfg.mode {
        Mode::Regions => regions(&pic, cfg, warm, scale),
        Mode::Ink => ink_shapes(&pic, cfg, warm, scale),
        Mode::Lines => line_strokes(&pic, cfg, warm, scale),
    };
    Ok(Traced { vector, ow, oh })
}

fn f32_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

#[derive(Default)]
struct Vectorize {
    /// The last frame's palette in Lab, and the mode it belongs to.
    warm: Vec<Lab>,
    warm_mode: Option<Mode>,
}

impl Node for Vectorize {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, _c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let d = inp.get("image").ok_or("`image` is required")?;
        let a = d.as_array()?;
        let cfg = config(p);
        if self.warm_mode != Some(cfg.mode) {
            self.warm.clear();
            self.warm_mode = Some(cfg.mode);
        }
        let t = trace_frame(a.shape(), a.as_bytes(), &cfg, &mut self.warm)?;
        let v = &t.vector;
        let img = render(v, t.ow, t.oh, cfg.weight);
        let rows = paths(v);
        let n = rows.len() / 4;
        let pal: Vec<f32> = v.palette.iter().flatten().copied().collect();
        let err = |e: goofi_core::GoofiError| e.to_string();
        out.set("out", Data::array_f32(vec![t.oh, t.ow, 4], f32_bytes(&img), Meta::new()).map_err(err)?);
        out.set("svg", Data::string(svg(v, t.ow, t.oh, &cfg), Meta::new()));
        out.set("paths", Data::array_f32(vec![n, 4], f32_bytes(&rows), Meta::new()).map_err(err)?);
        out.set("palette", Data::array_f32(vec![v.palette.len(), 3], f32_bytes(&pal), Meta::new()).map_err(err)?);
        Ok(())
    }
}

const fn number(name: &'static str, default: &'static [f64], min: f64, max: f64, int: bool, doc: &'static str) -> ParamDecl {
    ParamDecl {
        group: G,
        name,
        spec: ParamSpec::Num { default, min, max, int, options: &[], face: Face::Plain },
        expression: None,
        doc: Some(doc),
        section: 0,
        show: &[],
        role: None,
    }
}

/// The param in a section of the group, shown while `show` holds.
const fn placed(p: ParamDecl, section: u8, show: &'static [Show]) -> ParamDecl {
    ParamDecl { section, show, ..p }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: G,
        name: "mode",
        spec: ParamSpec::Str { default: "regions", options: &["regions", "ink", "lines"], refresh: false },
        expression: None,
        doc: Some(
            "What is traced. `regions` reduces any picture to `colors` colours and outlines every region of one \
             colour. `ink` finds the ink of a drawing on paper and outlines each ink colour. `lines` traces the same \
             ink as pen paths, one centre line per stroke, drawn at the pen width measured along it.",
        ),
        section: 0,
        show: &[],
        role: None,
    },
    number("size", &[512.0], 32.0, 4096.0, true, "Longest side of the working copy, in pixels. A smaller copy traces faster and coarser."),
    number("colors", &[8.0], 1.0, 32.0, true, "Palette size. In `regions` it is the count of colours. In `ink` and `lines` it is the most ink colours kept after near colours merge."),
    number("resolution", &[0.0], 0.0, 8192.0, true, "Longest side of `out`, in pixels. 0 draws at the size of the input."),
    placed(number("detail", &[0.8], 0.05, 8.0, false, "Largest distance from a curve to the traced edge, in working pixels. A larger value gives fewer and smoother curves."), 1, &[]),
    placed(number("smooth", &[1.2], 0.0, 6.0, false, "Gaussian smoothing of each traced edge before the fit, in working pixels. 0 keeps the pixel steps."), 1, &[]),
    placed(number("speckle", &[16.0], 0.0, 10000.0, false, "A region smaller than this many working pixels joins the neighbour it shares the longest border with."), 1, &[REGIONS]),
    placed(number("chroma", &[0.25], 0.0, 1.0, false, "Colour saturation of confident coloured ink. Raise it when paper texture or a tinted ground shows as ink."), 2, &[INK]),
    placed(number("dark", &[0.40], 0.0, 1.0, false, "Darkness of confident dark ink. Raise it when bleed-through or guide lines show as ink. Lower it for pencil."), 2, &[INK]),
    placed(number("merge", &[22.0], 0.0, 100.0, false, "Ink colours closer than this hue-weighted ΔE merge, so one marker stays one colour."), 2, &[INK]),
    placed(number("fill", &[1.8], 1.0, 6.0, false, "A shape thicker than this many pen widths is outlined as a fill."), 3, &[LINES]),
    placed(number("weight", &[1.0], 0.0, 8.0, false, "Drawn line width, as a multiple of the pen width measured in the drawing."), 3, &[LINES]),
];

static INPUTS: &[SlotDecl] = &[SlotDecl { name: "image", kind: SlotType::Array, trigger_process: true, multi: false, required: true }];
static OUTPUTS: &[OutputDecl] = &[
    OutputDecl { name: "out", kind: SlotType::Array },
    OutputDecl { name: "svg", kind: SlotType::String },
    OutputDecl { name: "paths", kind: SlotType::Array },
    OutputDecl { name: "palette", kind: SlotType::Array },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Image, Tag::Transform],
    doc: "An image as vector shapes, traced every frame and drawn back from its own curves.\n\
          `image` is an [H, W] or [H, W, C] frame in 0..1, or a texture through the graphics tap. `mode` sets the \
          trace. `regions` suits any picture. `ink` and `lines` suit a drawing on paper, where the paper is \
          flattened away and only the ink is traced. Edges are smoothed and fitted with cubic Béziers within \
          `detail` pixels, split at corners. `out` is the vector drawn as RGBA at `resolution`. It is opaque in \
          `regions` and has a clear ground in `ink` and `lines`, so it composites over its source. `svg` is the \
          same drawing as an SVG document. `paths` lists every drawn path as rows of x and y (0..1 of the frame, \
          y down), path index and palette index. In `lines` each path is a stroke from its upper-left end, in \
          drawing order. `palette` holds the colours as [K, 3] RGB. Each frame starts its palette from the last \
          one, so colours hold steady on video.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_sdk::signal::export!(Vectorize, MANIFEST);
