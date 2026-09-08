/* goofi
{ "doc": "the raster a spiking network is read as, scrolling\nOne row per neuron and one column per tick, oldest at the left: spikes as marks over the membrane potential as heat. The picture is the node's own, built up tick by tick, so a `value` frame is enough — a `block` frame folds to the strongest sample it holds. More neurons than rows fold into a band each.",
  "tags": ["image", "simulation"],
  "inputs": [
    {"name": "spikes", "kind": "ARRAY"},
    {"name": "potentials", "kind": "ARRAY"} ],
  "state": ["history"],
  "params": [
    {"group": "raster", "name": "layers", "kind": "str", "default": "both",
     "options": ["both", "spikes", "potentials"]},
    {"group": "raster", "name": "speed", "kind": "int", "default": 1, "min": 1, "max": 16},
    {"group": "raster", "name": "gain", "kind": "float", "default": 1.0, "min": 0.05, "max": 8.0} ] }
*/

// A band may cover twenty thousand neurons and a block a thousand steps, so each fold walks at
// most this many of them.
const MAX_FOLD: i32 = 32;

/// A dark-to-warm ramp: near black, through red, to pale yellow.
fn heat(v: f32) -> vec3f {
    let x = clamp(v, 0.0, 1.0);
    return clamp(vec3f(1.5 * x, 1.5 * x * x, 2.0 * x * x * x - 0.15), vec3f(0.0), vec3f(1.0));
}

fn wired(dims: vec2i, alpha: f32) -> bool {
    return dims.x > 1 || dims.y > 1 || alpha >= 0.5;
}

/// How many units the frame holds, and how many steps: a `value` frame is one row of units, a
/// `block` frame one ROW each with time along the columns.
fn units(dims: vec2i) -> i32 {
    return select(dims.y, dims.x, dims.y == 1);
}

fn steps(dims: vec2i) -> i32 {
    return select(dims.x, 1, dims.y == 1);
}

fn sample(tex: texture_2d<f32>, i: i32, t: i32, dims: vec2i) -> f32 {
    return textureLoad(tex, select(vec2i(t, i), vec2i(i, 0), dims.y == 1), 0).r;
}

/// The strongest sample in the band of units one raster row covers, over the whole block: a spike
/// anywhere in it shows rather than being missed between two ticks.
fn peak(tex: texture_2d<f32>, row: i32) -> f32 {
    let dims = vec2i(textureDimensions(tex));
    if !wired(dims, textureLoad(tex, vec2i(0, 0), 0).a) {
        return 0.0;
    }
    let n = units(dims);
    let rows = i32(resolution.y);
    let lo = row * n / rows;
    let hi = max(lo + 1, (row + 1) * n / rows);
    let cols = steps(dims);
    let over = max(1, (hi - lo + MAX_FOLD - 1) / MAX_FOLD);
    let along = max(1, (cols + MAX_FOLD - 1) / MAX_FOLD);
    var best = 0.0;
    for (var i = lo; i < min(hi, n); i = i + over) {
        for (var t = 0; t < cols; t = t + along) {
            best = max(best, sample(tex, i, t, dims));
        }
    }
    return best;
}

/// The mean of that band at the newest step: a potential is a level, not an event.
fn level(tex: texture_2d<f32>, row: i32) -> f32 {
    let dims = vec2i(textureDimensions(tex));
    if !wired(dims, textureLoad(tex, vec2i(0, 0), 0).a) {
        return 0.0;
    }
    let n = units(dims);
    let rows = i32(resolution.y);
    let lo = row * n / rows;
    let hi = max(lo + 1, (row + 1) * n / rows);
    let over = max(1, (hi - lo + MAX_FOLD - 1) / MAX_FOLD);
    var total = 0.0;
    var walked = 0.0;
    for (var i = lo; i < min(hi, n); i = i + over) {
        total = total + sample(tex, i, steps(dims) - 1, dims);
        walked = walked + 1.0;
    }
    return total / max(walked, 1.0);
}

fn next_history(uv: vec2f) -> vec4f {
    let at = vec2i(floor(uv * resolution));
    if frame == 0u {
        return vec4f(0.0, 0.0, 0.0, 1.0);
    }
    let step = clamp(p.speed, 1, i32(resolution.x));
    if at.x < i32(resolution.x) - step {
        return textureLoad(history, at + vec2i(step, 0), 0);
    }
    return vec4f(peak(spikes, at.y), level(potentials, at.y), 0.0, 1.0);
}

fn shade(uv: vec2f) -> vec4f {
    let cell = textureLoad(history, vec2i(floor(uv * resolution)), 0);
    var colour = vec3f(0.04, 0.05, 0.07);
    if p.layers != 1u {
        colour = heat(cell.g * p.gain);
    }
    if p.layers != 2u {
        colour = mix(colour, vec3f(0.98, 0.99, 1.0), clamp(cell.r, 0.0, 1.0));
    }
    return vec4f(colour, 1.0);
}
