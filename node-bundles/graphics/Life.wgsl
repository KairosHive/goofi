/* goofi
{ "doc": "a cellular automaton that keeps its own grid\nThe rule decides what lives; density and seed set the first generation. The grid is the node's own size, so a small one gives large cells.",
  "tags": ["image", "generator", "simulation"],
  "state": ["cells"],
  "params": [
    {"group": "life", "name": "rule", "kind": "str", "default": "conway", "options": ["conway", "highlife", "seeds", "daynight"]},
    {"group": "life", "name": "density", "kind": "float", "default": 0.35, "min": 0.0, "max": 1.0},
    {"group": "life", "name": "seed", "kind": "float", "default": 0.0, "min": 0.0, "max": 1000.0} ] }
*/
fn hash2(v: vec2f) -> f32 {
    let q = vec2u(vec2i(v) + 65536);
    var h = q.x * 1597334673u ^ q.y * 3812015801u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    return f32(h ^ (h >> 16u)) * (1.0 / 4294967296.0);
}

// The grid wraps, so a glider that leaves one edge arrives at the other.
fn alive(at: vec2i) -> f32 {
    let size = vec2i(resolution);
    let wrapped = ((at % size) + size) % size;
    return step(0.5, textureLoad(cells, wrapped, 0).r);
}

fn next_cells(uv: vec2f) -> vec4f {
    let at = vec2i(floor(uv * resolution));
    if frame == 0u {
        return vec4f(vec3f(step(1.0 - p.density, hash2(vec2f(at) + p.seed))), 1.0);
    }
    var neighbours = 0.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            if dx != 0 || dy != 0 {
                neighbours = neighbours + alive(at + vec2i(dx, dy));
            }
        }
    }
    // One bit per neighbour count: bit 3 set means "three neighbours does it".
    var birth = 8u;
    var survive = 12u;
    switch p.rule {
        case 1u: { birth = 72u; survive = 12u; }
        case 2u: { birth = 4u; survive = 0u; }
        case 3u: { birth = 456u; survive = 472u; }
        default: {}
    }
    let rule = select(birth, survive, alive(at) > 0.5);
    let lives = (rule & (1u << u32(neighbours))) != 0u;
    return vec4f(vec3f(select(0.0, 1.0, lives)), 1.0);
}

fn shade(uv: vec2f) -> vec4f {
    return next_cells(uv);
}
