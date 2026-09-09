/* goofi
{ "doc": "Signed Chladni and open-wave fields from Biotuner modes.\nWire HarmonicModes.modes to modes and HarmonicMorph.packed to harmonics. approach 0 evaluates rectangular cosine modes; approach 1 evaluates open directional waves. symmetry blends each cosine product toward its antisymmetric form. Fractional modes and approach blends are visual transitions, not closed-plate eigenmodes. The output is a signed scalar texture: color it with HarmonicInk.",
  "tags": ["image", "generator"],
  "inputs": [{"name": "modes", "kind": "ARRAY"}, {"name": "harmonics", "kind": "ARRAY"}],
  "params": [
    {"group": "field", "name": "approach", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0, "doc": "Plate modes to open directional interference."},
    {"group": "field", "name": "symmetry", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0, "doc": "Cosine products to antisymmetric swapped products."},
    {"group": "field", "name": "period", "kind": "float", "default": 0.35, "min": 0.04, "max": 4.0, "doc": "Base period across the normalized open-wave image."},
    {"group": "field", "name": "directions", "kind": "int", "default": 5, "min": 2, "max": 12, "doc": "Rotational order of open directional waves."},
    {"group": "field", "name": "rotation", "kind": "float", "default": 0.0, "min": -1000.0, "max": 1000.0, "doc": "Open wave direction offset, radians."},
    {"group": "common", "name": "width", "kind": "int", "default": 256, "min": 0, "max": 4096},
    {"group": "common", "name": "height", "kind": "int", "default": 256, "min": 0, "max": 4096}
  ] }
*/

fn shade(uv: vec2f) -> vec4f {
    let md = textureDimensions(modes);
    let hd = textureDimensions(harmonics);
    // Match Biotuner's inclusive grid for the plate endpoint.
    let pos = floor(uv * resolution) / max(resolution - 1.0, vec2f(1.0));
    var plate = 0.0; var wave = 0.0;
    var plate_weight = 0.0; var wave_weight = 0.0;
    for (var i = 0; i < 32; i = i + 1) {
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
