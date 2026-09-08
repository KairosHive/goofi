/* goofi
{ "doc": "the audio plane as a running picture\nThe door in from a stream, which has a history where a frame has none: every render leaves a column standing and the picture walks left. `audio/mode` says what a column holds — the band its samples span, the frame itself stacked down it, or, on a trace that fades instead of walking, two channels against each other. Set common/width and height, or it is 1024 square.",
  "tags": ["image", "generator"],
  "state": ["history"],
  "inputs": [{"name": "input", "kind": "AUDIO"}],
  "params": [
    {"group": "audio", "name": "mode", "kind": "str", "default": "scroll",
     "options": ["scroll", "waterfall", "phase"]},
    {"group": "audio", "name": "autoscale", "kind": "bool", "default": true},
    {"group": "audio", "name": "range", "kind": "float", "default": 1.0, "min": 0.000001, "max": 1000000.0},
    {"group": "audio", "name": "decay", "kind": "float", "default": 0.94, "min": 0.0, "max": 1.0},
    {"group": "audio", "name": "thickness", "kind": "float", "default": 1.5, "min": 0.5, "max": 8.0} ] }
*/

// A column folds at most this many samples, and a trace walks at most this many points: what
// keeps a frame of a million samples from costing a million texel reads at every texel.
const MAX_FOLD: i32 = 64;
const MAX_WALK: i32 = 256;

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

/// Half the window the drawing maps onto the frame: the widest the samples went, or the one
/// asked for. Audio is bipolar by convention, so one number states both ends.
fn half_span() -> f32 {
    if (p.autoscale != 0u) {
        return max(max(abs(p.input_lo), abs(p.input_hi)), 1e-6);
    }
    return max(p.range, 1e-6);
}

/// The row a value falls on, in texels.
fn row_of(v: f32, half: f32) -> f32 {
    return (1.0 - (v / half + 1.0) * 0.5) * resolution.y;
}

/// What the last render left at `at`, and nothing before the first one.
fn held(at: vec2i) -> vec4f {
    let size = vec2i(resolution);
    if (frame == 0u || at.x < 0 || at.y < 0 || at.x >= size.x || at.y >= size.y) {
        return vec4f(0.0);
    }
    return textureLoad(history, at, 0);
}

/// The standing column: each channel's band between the lowest and highest sample of the frame.
fn column_scroll(y: f32, n: i32, chans: i32, half: f32) -> vec4f {
    var out = vec4f(0.0);
    let stride = max(1, (n + MAX_FOLD - 1) / MAX_FOLD);
    for (var c = 0; c < chans; c = c + 1) {
        var lo = 1e30;
        var hi = -1e30;
        var i = 0;
        loop {
            if (i >= n) { break; }
            let v = at(i, c);
            lo = min(lo, v);
            hi = max(hi, v);
            i = i + stride;
        }
        let band = vec2f(row_of(hi, half), row_of(lo, half));
        let away = max(max(band.x - y, y - band.y), 0.0);
        out = ink(out, series(c), clamp(p.thickness + 0.5 - away, 0.0, 1.0));
    }
    return out;
}

/// The standing column: the frame laid down it, one band per channel, sample 0 at the bottom —
/// so a spectrum stacks into a spectrogram.
fn column_waterfall(y: f32, n: i32, chans: i32, half: f32) -> vec4f {
    let band = resolution.y / f32(chans);
    let c = clamp(i32(floor(y / band)), 0, chans - 1);
    let up = 1.0 - fract(y / band);
    let i = clamp(i32(up * f32(n - 1) + 0.5), 0, n - 1);
    let v = clamp(abs(at(i, c)) / half, 0.0, 1.0);
    return vec4f(series(c), v);
}

/// How far a point is from a segment, squared — the whole of what draws a path.
fn to_segment(here: vec2f, a: vec2f, b: vec2f) -> f32 {
    let run = b - a;
    let t = clamp(dot(here - a, run) / max(dot(run, run), 1e-6), 0.0, 1.0);
    let off = here - (a + run * t);
    return dot(off, off);
}

/// Channel 0 across and channel 1 up; a stream with one channel is drawn against its own next
/// sample, which is the portrait a single lane has.
fn point_of(t: i32, n: i32, chans: i32, half: f32) -> vec2f {
    let a = at(t, 0);
    let b = select(at(min(t + 1, n - 1), 0), at(t, 1), chans > 1);
    return vec2f((a / half + 1.0) * 0.5 * resolution.x, row_of(b, half));
}

fn phase_cover(here: vec2f, n: i32, chans: i32, half: f32) -> f32 {
    let stride = max(1, (n + MAX_WALK - 1) / MAX_WALK);
    var prev = point_of(0, n, chans, half);
    var near = 1e30;
    var t = stride;
    loop {
        if (t >= n) { break; }
        let now = point_of(t, n, chans, half);
        near = min(near, to_segment(here, prev, now));
        prev = now;
        t = t + stride;
    }
    near = min(near, to_segment(here, prev, point_of(n - 1, n, chans, half)));
    return clamp(p.thickness + 0.5 - sqrt(near), 0.0, 1.0);
}

/// One texel of this render — the output and the history are the one picture, so the walk and
/// the fade have a single owner and cannot fall a column apart.
fn cell(uv: vec2f) -> vec4f {
    let dims = vec2i(textureDimensions(input));
    // Nothing is wired: the one shared transparent texel, which is no stream at all.
    if (dims.x == 1 && dims.y == 1 && textureLoad(input, vec2i(0, 0), 0).a < 0.5) {
        return vec4f(0.0);
    }
    let here = uv * resolution;
    let spot = vec2i(floor(here));
    let half = half_span();
    if (p.mode == 2u) {
        let under = held(spot);
        return ink(vec4f(under.rgb, under.a * p.decay), series(0), phase_cover(here, dims.x, dims.y, half));
    }
    if (spot.x < i32(resolution.x) - 1) {
        return held(spot + vec2i(1, 0));
    }
    if (p.mode == 1u) {
        return column_waterfall(here.y, dims.x, dims.y, half);
    }
    return column_scroll(here.y, dims.x, dims.y, half);
}

fn shade(uv: vec2f) -> vec4f {
    return cell(uv);
}

fn next_history(uv: vec2f) -> vec4f {
    return cell(uv);
}
