/* goofi
{ "doc": "Chladni displacement and nodal density from Biotuner modes.\nWire HarmonicModes.modes to modes and HarmonicMorph.packed to harmonics. For the cymatics notebook, select chord pairs in HarmonicModes, symmetry 1 here, output nodal, and density_symmetry d4_max. Whole-chord pairwise cosine products become exp(-w²/σ²), then eight square density transforms are united by maximum or averaged. D4 never changes signed displacement. Sigma is explicit, initially 0.05. approach 1 evaluates open directional waves. Fractional modes and field blends are visual transitions, not closed-plate eigenmodes. Signed output uses normal HarmonicInk; density output needs Ink style density or Relief input_kind density. A displacement buffer adds one graphics frame of latency without accumulation.",
  "tags": ["image", "generator"],
  "state": ["displacement"],
  "inputs": [{"name": "modes", "kind": "ARRAY"}, {"name": "harmonics", "kind": "ARRAY"}],
  "params": [
    {"group": "field", "name": "output", "kind": "str", "default": "signed", "options": ["signed", "nodal", "antinodal"], "doc": "Signed displacement or Biotuner's exp(-w²/σ²) density. Use nodal for the cymatics notebook."},
    {"group": "field", "name": "density_symmetry", "kind": "str", "default": "d4_max", "options": ["none", "d4_max", "d4_sum"], "doc": "Union (max) or average of eight square density transforms. Applied after the Gaussian, never to the signed field. Non-square grids skip D4, as in Biotuner."},
    {"group": "field", "name": "sigma", "kind": "float", "default": 0.05, "min": 0.001, "max": 0.4, "doc": "Nodal stripe width in field units. Fixed 0.05 matches the notebook's explicit sand example."},
    {"group": "field", "name": "approach", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0, "doc": "Plate modes to open directional interference."},
    {"group": "field", "name": "symmetry", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0, "doc": "Cosine products to antisymmetric swapped products."},
    {"group": "field", "name": "period", "kind": "float", "default": 0.35, "min": 0.04, "max": 4.0, "doc": "Base period across the normalized open-wave image."},
    {"group": "field", "name": "directions", "kind": "int", "default": 5, "min": 2, "max": 12, "doc": "Rotational order of open directional waves."},
    {"group": "field", "name": "rotation", "kind": "float", "default": 0.0, "min": -1000.0, "max": 1000.0, "doc": "Open wave direction offset, radians."},
    {"group": "common", "name": "width", "kind": "int", "default": 256, "min": 0, "max": 4096},
    {"group": "common", "name": "height", "kind": "int", "default": 256, "min": 0, "max": 4096}
  ] }
*/

fn next_displacement(uv: vec2f) -> vec4f {
    let md = textureDimensions(modes);
    let hd = textureDimensions(harmonics);
    // Match Biotuner's inclusive grid for the plate endpoint.
    let pos = floor(uv * resolution) / max(resolution - 1.0, vec2f(1.0));
    var plate = 0.0; var wave = 0.0;
    var plate_weight = 0.0; var wave_weight = 0.0;
    for (var i = 0; i < 64; i = i + 1) {
        if md.y >= 4u && u32(i) < md.x && p.approach < 1.0 {
            let weight = max(textureLoad(modes, vec2i(i, 2), 0).r, 0.0);
            if weight > 0.0 {
                let m = textureLoad(modes, vec2i(i, 0), 0).r;
                let n = textureLoad(modes, vec2i(i, 1), 0).r;
                let phase = textureLoad(modes, vec2i(i, 3), 0).r;
                let forward = cos(m * 3.14159265359 * pos.x) * cos(n * 3.14159265359 * pos.y);
                let backward = cos(n * 3.14159265359 * pos.x) * cos(m * 3.14159265359 * pos.y);
                plate = plate + weight * (forward - p.symmetry * backward) * cos(phase);
                plate_weight = plate_weight + weight;
            }
        }
        if hd.y >= 4u && u32(i) < hd.x && p.approach > 0.0 {
            let weight = max(textureLoad(harmonics, vec2i(i, 1), 0).r, 0.0);
            if weight > 0.0 {
                let ratio = textureLoad(harmonics, vec2i(i, 0), 0).r;
                let phase = textureLoad(harmonics, vec2i(i, 2), 0).r;
                for (var j = 0; j < p.directions; j = j + 1) {
                    let angle = 6.28318530718 * f32(j) / f32(p.directions) + p.rotation;
                    let along = dot(pos - 0.5, vec2f(cos(angle), sin(angle)));
                    wave = wave + weight * cos(6.28318530718 * ratio * along / p.period + phase) / f32(p.directions);
                }
                wave_weight = wave_weight + weight;
            }
        }
    }
    let value = mix(plate, wave, p.approach);
    let coverage = select(0.0, 1.0, mix(plate_weight, wave_weight, p.approach) > 0.0);
    return vec4f(vec3f(value), coverage);
}

fn density_at(at: vec2i) -> f32 {
    let w = textureLoad(displacement, at, 0).r;
    let nodal = exp(-w*w/(p.sigma*p.sigma));
    return select(nodal, 1.0-nodal, p.output == 2u);
}

fn shade(uv: vec2f) -> vec4f {
    let dims = vec2i(textureDimensions(displacement));
    let at = clamp(vec2i(uv*vec2f(dims)), vec2i(0), dims-1);
    let field = textureLoad(displacement, at, 0);
    if p.output == 0u { return field; }
    var density = density_at(at);
    if p.density_symmetry != 0u && dims.x == dims.y {
        let end = dims.x-1;
        let orbit = array<vec2i, 8>(at, vec2i(end-at.y, at.x),
            vec2i(end-at.x, end-at.y), vec2i(at.y, end-at.x),
            vec2i(end-at.x, at.y), vec2i(at.x, end-at.y),
            vec2i(at.y, at.x), vec2i(end-at.y, end-at.x));
        density = 0.0;
        for (var i = 0; i < 8; i += 1) {
            let d = density_at(orbit[i]);
            density = select(max(density, d), density+d/8.0, p.density_symmetry == 2u);
        }
    }
    return vec4f(vec3f(density), field.a);
}
