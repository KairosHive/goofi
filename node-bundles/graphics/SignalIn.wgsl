/* goofi
{ "doc": "a signal frame as a texture\nThe door in from the signal plane. `signal/mode` says what the frame becomes: its own texels — [N] is one row, [H, W] is gray, [H, W, C] keeps the channels it has — or the line plot or the trajectory a viewer draws of it, on transparent ground so it composites. A [C, N] frame is C series; a trajectory pairs the channels, one against the other. Set common/width and height, or it is 1024 square.",
  "tags": ["image", "generator"],
  "inputs": [{"name": "input", "kind": "ARRAY"}],
  "params": [
    {"group": "signal", "name": "mode", "kind": "str", "default": "texture",
     "options": ["texture", "line", "trajectory"]},
    {"group": "signal", "name": "autoscale", "kind": "bool", "default": true},
    {"group": "signal", "name": "min", "kind": "float", "default": -1.0, "min": -1000000000.0, "max": 1000000000.0},
    {"group": "signal", "name": "max", "kind": "float", "default": 1.0, "min": -1000000000.0, "max": 1000000000.0},
    {"group": "signal", "name": "log_x", "kind": "bool", "default": false},
    {"group": "signal", "name": "log_y", "kind": "bool", "default": false},
    {"group": "signal", "name": "points", "kind": "float", "default": 0.0, "min": 0.0, "max": 12.0},
    {"group": "signal", "name": "thickness", "kind": "float", "default": 1.5, "min": 0.5, "max": 8.0} ] }
*/

// Half a path's width, and the dot that says where a trajectory is now.
const HEAD: f32 = 2.5;
// A column folds at most this many samples, and a path walks at most this many points: what keeps
// a million-sample frame from costing a million texel reads at every texel.
const MAX_FOLD: i32 = 48;
const MAX_WALK: i32 = 192;
const MAX_PAIRS: i32 = 8;

/// The colours a series is drawn in, in the viewers' own order.
fn series(k: i32) -> vec3f {
    switch (k % 8) {
        case 1: { return vec3f(0.710, 0.549, 1.000); }
        case 2: { return vec3f(0.365, 0.816, 0.604); }
        case 3: { return vec3f(1.000, 0.718, 0.380); }
        case 4: { return vec3f(1.000, 0.478, 0.635); }
        case 5: { return vec3f(0.604, 0.639, 0.702); }
        case 6: { return vec3f(0.773, 0.784, 0.839); }
        case 7: { return vec3f(0.431, 0.463, 0.525); }
        default: { return vec3f(0.478, 0.718, 1.000); }
    }
}

fn ink(under: vec4f, colour: vec3f, cover: f32) -> vec4f {
    return vec4f(mix(under.rgb, colour, cover), max(under.a, cover));
}

/// Sample `i` of channel `c`, as the upload left it.
fn at(i: i32, c: i32) -> f32 {
    return textureLoad(input, vec2i(i, c), 0).r;
}

/// Where a value sits on the axis: itself, or its decade.
fn ordinate(v: f32) -> f32 {
    if (p.log_y != 0u) {
        return log(max(v, 1e-12)) / log(10.0);
    }
    return v;
}

/// The range the drawing maps onto the frame: the data's own, or the one asked for. A log axis
/// takes a positive window, and a degenerate one becomes a unit window at its centre.
fn span() -> vec2f {
    var lo = p.min;
    var hi = p.max;
    if (p.autoscale != 0u) {
        lo = p.input_lo;
        hi = p.input_hi;
    }
    if (p.log_y != 0u) {
        hi = select(1.0, hi, hi > 0.0);
        lo = select(hi * 1e-3, lo, lo > 0.0 && lo < hi);
    }
    lo = ordinate(lo);
    hi = ordinate(hi);
    if (hi > lo) {
        return vec2f(lo, hi);
    }
    let centre = (lo + hi) * 0.5;
    return vec2f(centre - 1.0, centre + 1.0);
}

/// Where sample `i` of `n` sits across the frame, and the way back from a point on it.
fn abscissa(i: f32, n: f32) -> f32 {
    if (p.log_x != 0u) {
        return log(i + 1.0) / log(n);
    }
    return i / max(n - 1.0, 1.0);
}

fn sample_at(x: f32, n: f32) -> f32 {
    let t = clamp(x, 0.0, 1.0);
    if (p.log_x != 0u) {
        return pow(n, t) - 1.0;
    }
    return t * max(n - 1.0, 1.0);
}

/// The row a value falls on, in texels.
fn row_of(v: f32, sp: vec2f) -> f32 {
    return (1.0 - (v - sp.x) / (sp.y - sp.x)) * resolution.y;
}

/// The value the path holds part way between two samples, on the axis it is drawn on.
fn along(x: f32, c: i32, n: i32) -> f32 {
    let t = clamp(x, 0.0, f32(n - 1));
    let i = i32(floor(t));
    let j = min(i + 1, n - 1);
    return mix(ordinate(at(i, c)), ordinate(at(j, c)), fract(t));
}

/// One channel's line: the band this column of texels spans, which is its two edges and every
/// sample between them — the min/max fold a viewer draws, resolved per texel.
fn line_channel(uv: vec2f, c: i32, n: i32, sp: vec2f) -> f32 {
    let half = 0.5 / resolution.x;
    let a = sample_at(uv.x - half, f32(n));
    let b = sample_at(uv.x + half, f32(n));
    var lo = min(along(a, c, n), along(b, c, n));
    var hi = max(along(a, c, n), along(b, c, n));
    let here = uv * resolution;
    var dot_cover = 0.0;
    let first = max(i32(ceil(a)), 0);
    let last = min(i32(floor(b)), n - 1);
    let stride = max(1, (last - first + MAX_FOLD) / MAX_FOLD);
    var i = first;
    loop {
        if (i > last) { break; }
        let v = ordinate(at(i, c));
        lo = min(lo, v);
        hi = max(hi, v);
        if (p.points > 0.0) {
            let dot = vec2f(abscissa(f32(i), f32(n)) * resolution.x, row_of(v, sp));
            dot_cover = max(dot_cover, clamp(p.points + 0.5 - distance(here, dot), 0.0, 1.0));
        }
        i = i + stride;
    }
    let band = vec2f(row_of(hi, sp), row_of(lo, sp));
    let away = max(max(band.x - here.y, here.y - band.y), 0.0);
    return max(clamp(p.thickness + 0.5 - away, 0.0, 1.0), dot_cover);
}

/// How far a point is from a segment, squared — the whole of what draws a path.
fn to_segment(here: vec2f, a: vec2f, b: vec2f) -> f32 {
    let run = b - a;
    let t = clamp(dot(here - a, run) / max(dot(run, run), 1e-6), 0.0, 1.0);
    let off = here - (a + run * t);
    return dot(off, off);
}

/// Channel `i` across and channel `j` up, both on the one range, so the shape is not distorted.
fn point_of(t: i32, i: int_pair, sp: vec2f) -> vec2f {
    let x = (ordinate(at(t, i.a)) - sp.x) / (sp.y - sp.x);
    return vec2f(x * resolution.x, row_of(ordinate(at(t, i.b)), sp));
}

struct int_pair {
    a: i32,
    b: i32,
}

fn trajectory_pair(here: vec2f, pair: int_pair, n: i32, sp: vec2f, budget: i32) -> f32 {
    let stride = max(1, (n + budget - 1) / budget);
    var prev = point_of(0, pair, sp);
    var near = 1e30;
    var t = stride;
    loop {
        if (t >= n) { break; }
        let now = point_of(t, pair, sp);
        near = min(near, to_segment(here, prev, now));
        prev = now;
        t = t + stride;
    }
    let last = point_of(n - 1, pair, sp);
    near = min(near, to_segment(here, prev, last));
    let path = clamp(p.thickness + 0.5 - sqrt(near), 0.0, 1.0);
    let head = clamp(max(p.points, HEAD) + 0.5 - distance(here, last), 0.0, 1.0);
    return max(path, head);
}

fn shade(uv: vec2f) -> vec4f {
    if (p.mode == 0u) {
        return textureSample(input, samp, uv);
    }
    let dims = vec2i(textureDimensions(input));
    // Nothing is wired: the one shared transparent texel. A plot of it is no plot.
    if (dims.x == 1 && dims.y == 1 && textureLoad(input, vec2i(0, 0), 0).a < 0.5) {
        return vec4f(0.0);
    }
    let sp = span();
    var out = vec4f(0.0);
    if (p.mode == 1u) {
        for (var c = 0; c < dims.y; c = c + 1) {
            out = ink(out, series(c), line_channel(uv, c, dims.x, sp));
        }
        return out;
    }
    var pairs = 0;
    for (var i = 0; i < dims.y; i = i + 1) {
        for (var j = i + 1; j < dims.y; j = j + 1) {
            pairs = pairs + 1;
        }
    }
    pairs = min(pairs, MAX_PAIRS);
    if (pairs == 0) {
        return out;
    }
    let budget = max(2, MAX_WALK / pairs);
    let here = uv * resolution;
    var k = 0;
    for (var i = 0; i < dims.y; i = i + 1) {
        for (var j = i + 1; j < dims.y; j = j + 1) {
            if (k < pairs) {
                out = ink(out, series(k), trajectory_pair(here, int_pair(i, j), dims.x, sp, budget));
            }
            k = k + 1;
        }
    }
    return out;
}
