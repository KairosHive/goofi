/* goofi
{ "doc": "a cellular automaton whose rule is a small network\nEach cell sees itself, the slope around it and the curvature, in four channels; a learned layer says what to become. Growth spreads from what is already alive. Wire 68 weights into `weights` to run a trained rule; with nothing wired it draws its own from `seed`.",
  "tags": ["image", "generator", "simulation", "ml"],
  "state": ["cells"],
  "inputs": [{"name": "weights", "kind": "ARRAY"}],
  "params": [
    {"group": "neuralca", "name": "rate", "kind": "float", "default": 0.3, "min": 0.001, "max": 1.0},
    {"group": "neuralca", "name": "fire", "kind": "float", "default": 0.5, "min": 0.05, "max": 1.0},
    {"group": "neuralca", "name": "start", "kind": "str", "default": "noise", "options": ["noise", "dot"]},
    {"group": "neuralca", "name": "density", "kind": "float", "default": 0.3, "min": 0.0, "max": 1.0},
    {"group": "neuralca", "name": "scale", "kind": "float", "default": 4.0, "min": 1.0, "max": 32.0},
    {"group": "neuralca", "name": "seed", "kind": "float", "default": 0.0, "min": 0.0, "max": 1000.0} ] }
*/
fn hash1(i: f32) -> f32 {
    var h = u32(i32(i) + 65536) * 1597334673u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) * (1.0 / 4294967296.0);
}

fn hash3(v: vec3f) -> f32 {
    let q = vec3u(vec3i(v) + 65536);
    var h = q.x * 1597334673u ^ q.y * 3812015801u ^ q.z * 2654435761u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) * (1.0 / 4294967296.0);
}

fn state(at: vec2i) -> vec4f {
    let size = vec2i(resolution);
    return textureLoad(cells, ((at % size) + size) % size, 0);
}

// Weight `i` of the 68: sixty-four for the layer, in output-major order, then four biases. An
// unwired input is one texel wide, and that is what says to draw the rule instead of reading one.
// The drawn ones are scaled by the fan-in so the first step does not saturate the tanh.
fn weight(i: i32) -> f32 {
    if i32(textureDimensions(weights).x) >= 68 {
        return textureLoad(weights, vec2i(i, 0), 0).r;
    }
    let bias = i >= 64;
    return (hash1(f32(i) + p.seed) * 2.0 - 1.0) * select(0.75, 0.05, bias);
}

// The fourth channel is aliveness. A cell with no living neighbour holds at zero, which is what
// makes a pattern GROW from its seed instead of the bias filling the whole grid with one colour.
fn awake(at: vec2i) -> bool {
    var most = 0.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            most = max(most, state(at + vec2i(dx, dy)).w);
        }
    }
    return most > 0.1;
}

fn seeded(at: vec2i) -> vec4f {
    if p.start == 1u {
        let d = length(vec2f(at) - resolution * 0.5);
        return vec4f(vec3f(step(d, 2.0)), step(d, 2.0));
    }
    // Blocks rather than pixels: a single live pixel has nothing around it to read.
    let block = floor(vec2f(at) / max(p.scale, 1.0));
    let on = step(1.0 - p.density, hash3(vec3f(block, p.seed)));
    return vec4f(hash3(vec3f(block, p.seed + 1.0)) * on, hash3(vec3f(block, p.seed + 2.0)) * on, hash3(vec3f(block, p.seed + 3.0)) * on, on);
}

fn next_cells(uv: vec2f) -> vec4f {
    let at = vec2i(floor(uv * resolution));
    if frame == 0u {
        return seeded(at);
    }
    let here = state(at);
    if !awake(at) {
        return vec4f(0.0);
    }
    // Sobel slopes and the Laplacian: direction and curvature, which is what lets a rule diffuse
    // rather than only saturate.
    let dx = (state(at + vec2i(1, -1)) + 2.0 * state(at + vec2i(1, 0)) + state(at + vec2i(1, 1))
        - state(at + vec2i(-1, -1)) - 2.0 * state(at + vec2i(-1, 0)) - state(at + vec2i(-1, 1))) / 8.0;
    let dy = (state(at + vec2i(-1, 1)) + 2.0 * state(at + vec2i(0, 1)) + state(at + vec2i(1, 1))
        - state(at + vec2i(-1, -1)) - 2.0 * state(at + vec2i(0, -1)) - state(at + vec2i(1, -1))) / 8.0;
    let lap = (state(at + vec2i(1, 0)) + state(at + vec2i(-1, 0)) + state(at + vec2i(0, 1)) + state(at + vec2i(0, -1))
        - 4.0 * here) / 4.0;
    var seen = array<f32, 16>(
        here.x, here.y, here.z, here.w,
        dx.x, dx.y, dx.z, dx.w,
        dy.x, dy.y, dy.z, dy.w,
        lap.x, lap.y, lap.z, lap.w,
    );
    var delta = vec4f(0.0);
    for (var o = 0; o < 4; o++) {
        var sum = weight(64 + o);
        for (var i = 0; i < 16; i++) {
            sum = sum + weight(o * 16 + i) * seen[i];
        }
        delta[o] = tanh(sum);
    }
    // Cells step at random rather than in lockstep, which is what keeps the grid from moving as
    // one block.
    let fires = step(1.0 - p.fire, hash3(vec3f(vec2f(at), f32(frame) + p.seed)));
    return clamp(here + p.rate * fires * delta, vec4f(-1.0), vec4f(1.0));
}

fn shade(uv: vec2f) -> vec4f {
    let s = state(vec2i(floor(uv * resolution)));
    return vec4f(clamp(s.rgb * 0.5 + 0.5, vec3f(0.0), vec3f(1.0)), 1.0);
}
