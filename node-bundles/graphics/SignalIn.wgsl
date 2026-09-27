/* goofi
{ "doc": "a signal frame as a texture\nThe door in from the signal plane. `signal/mode` says what the frame becomes: its own texels — [N] is one row, [H, W] is gray, [H, W, C] keeps the channels it has — or the line plot or the trajectory a viewer draws of it, on transparent ground so it composites. A texel is the value on the range, through the colormap where the frame has one channel. A [C, N] frame is C series; a trajectory pairs the channels, one against the other. Set common/width and height, or the texture is the frame's own size.",
  "tags": ["image", "generator"],
  "inputs": [{"name": "input", "kind": "ARRAY"}],
  "params": [
    {"group": "signal", "name": "mode", "kind": "str", "default": "texture",
     "options": ["texture", "line", "trajectory"]},
    {"group": "signal", "name": "colormap", "kind": "str", "default": "viridis",
     "options": ["viridis", "magma", "plasma", "gray", "jet", "coolwarm"],
     "show": {"param": "mode", "any_of": ["texture"]}},
    {"group": "signal", "name": "autoscale", "kind": "bool", "default": true},
    {"group": "signal", "name": "min", "kind": "float", "default": -1.0, "min": -10.0, "max": 10.0,
     "show": {"param": "autoscale", "any_of": ["false"]}},
    {"group": "signal", "name": "max", "kind": "float", "default": 1.0, "min": -10.0, "max": 10.0,
     "show": {"param": "autoscale", "any_of": ["false"]}},
    {"group": "signal", "name": "log_x", "kind": "bool", "default": false,
     "show": {"param": "mode", "any_of": ["line"]}},
    {"group": "signal", "name": "log_y", "kind": "bool", "default": false,
     "show": {"param": "mode", "any_of": ["line", "trajectory"]}},
    {"group": "signal", "name": "points", "kind": "float", "default": 0.0, "min": 0.0, "max": 12.0,
     "show": {"param": "mode", "any_of": ["line", "trajectory"]}},
    {"group": "signal", "name": "thickness", "kind": "float", "default": 1.5, "min": 0.5, "max": 8.0,
     "show": {"param": "mode", "any_of": ["line", "trajectory"]}} ] }
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

/// The stops of colormap `k`, in the viewers' menu order, and how many of them are set.
fn stops(k: u32) -> array<vec3f, 7> {
    switch (k) {
        case 1u: { return array<vec3f, 7>(vec3f(0.000, 0.000, 0.016), vec3f(0.318, 0.071, 0.486), vec3f(0.718, 0.216, 0.475), vec3f(0.988, 0.537, 0.380), vec3f(0.988, 0.992, 0.749), vec3f(0.0), vec3f(0.0)); }
        case 2u: { return array<vec3f, 7>(vec3f(0.051, 0.031, 0.529), vec3f(0.494, 0.012, 0.659), vec3f(0.800, 0.278, 0.471), vec3f(0.973, 0.584, 0.251), vec3f(0.941, 0.976, 0.129), vec3f(0.0), vec3f(0.0)); }
        case 3u: { return array<vec3f, 7>(vec3f(0.0), vec3f(1.0), vec3f(0.0), vec3f(0.0), vec3f(0.0), vec3f(0.0), vec3f(0.0)); }
        case 4u: { return array<vec3f, 7>(vec3f(0.000, 0.000, 0.514), vec3f(0.000, 0.235, 0.667), vec3f(0.020, 1.000, 1.000), vec3f(1.000, 1.000, 0.000), vec3f(0.980, 0.000, 0.000), vec3f(0.502, 0.000, 0.000), vec3f(0.0)); }
        case 5u: { return array<vec3f, 7>(vec3f(0.231, 0.298, 0.753), vec3f(0.471, 0.627, 0.961), vec3f(0.753, 0.804, 0.902), vec3f(0.867, 0.863, 0.863), vec3f(0.941, 0.706, 0.620), vec3f(0.902, 0.431, 0.353), vec3f(0.706, 0.016, 0.149)); }
        default: { return array<vec3f, 7>(vec3f(0.267, 0.004, 0.329), vec3f(0.231, 0.322, 0.545), vec3f(0.129, 0.565, 0.549), vec3f(0.365, 0.788, 0.388), vec3f(0.992, 0.906, 0.145), vec3f(0.0), vec3f(0.0)); }
    }
}

fn stop_count(k: u32) -> i32 {
    switch (k) {
        case 3u: { return 2; }
        case 4u: { return 6; }
        case 5u: { return 7; }
        default: { return 5; }
    }
}

/// Where `t` of the unit range falls on colormap `k`: the viewers' own LUT, interpolated.
fn colormap(k: u32, t: f32) -> vec3f {
    var s = stops(k);
    let n = stop_count(k);
    let x = clamp(t, 0.0, 1.0) * f32(n - 1);
    let i = min(i32(floor(x)), n - 2);
    return mix(s[i], s[i + 1], x - f32(i));
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

/// The range a value is read against: the data's own, or the one asked for.
fn window() -> vec2f {
    if (p.autoscale != 0u) {
        return vec2f(p.input_lo, p.input_hi);
    }
    return vec2f(p.min, p.max);
}

/// The range the drawing maps onto the frame. A log axis takes a positive window, and a
/// degenerate one becomes a unit window at its centre.
fn span() -> vec2f {
    var lo = window().x;
    var hi = window().y;
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

/// The frame's own texels, each read against the range: a one-channel value through the colormap,
/// a colour frame's channels as they are, and its alpha untouched.
fn texel(uv: vec2f) -> vec4f {
    let v = textureSample(input, samp, uv);
    let w = window();
    let n = (v.rgb - w.x) / max(w.y - w.x, 1e-12);
    if (p.input_channels < 2.5) {
        return vec4f(colormap(p.colormap, n.r), v.a);
    }
    return vec4f(clamp(n, vec3f(0.0), vec3f(1.0)), v.a);
}

fn shade(uv: vec2f) -> vec4f {
    let dims = vec2i(textureDimensions(input));
    // Nothing is wired: the one shared transparent texel. A plot of it is no plot.
    if (dims.x == 1 && dims.y == 1 && textureLoad(input, vec2i(0, 0), 0).a < 0.5) {
        return vec4f(0.0);
    }
    if (p.mode == 0u) {
        return texel(uv);
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
