/* goofi
{ "doc": "Scale and shift texture channels, then remap their range.\nThe order is (input + pre_add) * multiply + post_add, followed by range mapping. RGB leaves alpha unchanged; alpha leaves RGB unchanged; RGBA applies the same math to all four channels. A zero-width source range gives to_low. Reversed ranges are supported. Bounds use the sorted target endpoints: clamp holds values at the edges; wrap repeats the range; fold halves or doubles positive values into a positive range, as in signal Math. Fold ranges narrower than an octave can leave values outside. Values are not clamped unless clamp is selected.",
  "tags": ["image", "transform"],
  "inputs": [{"name": "input", "kind": "TEXTURE"}],
  "params": [
    {"group": "math", "name": "channels", "kind": "str", "default": "rgb", "options": ["rgb", "rgba", "alpha"]},
    {"group": "math", "name": "pre_add", "kind": "float", "default": 0.0, "min": -1000000000.0, "max": 1000000000.0},
    {"group": "math", "name": "multiply", "kind": "float", "default": 1.0, "min": -1000000000.0, "max": 1000000000.0},
    {"group": "math", "name": "post_add", "kind": "float", "default": 0.0, "min": -1000000000.0, "max": 1000000000.0},
    {"group": "range", "name": "from_low", "kind": "float", "default": 0.0, "min": -1000000000.0, "max": 1000000000.0},
    {"group": "range", "name": "from_high", "kind": "float", "default": 1.0, "min": -1000000000.0, "max": 1000000000.0},
    {"group": "range", "name": "to_low", "kind": "float", "default": 0.0, "min": -1000000000.0, "max": 1000000000.0},
    {"group": "range", "name": "to_high", "kind": "float", "default": 1.0, "min": -1000000000.0, "max": 1000000000.0},
    {"group": "range", "name": "bound", "kind": "str", "default": "none", "options": ["none", "clamp", "wrap", "fold"]} ] }
*/
fn map_channel(value: f32) -> f32 {
    let span = p.from_high - p.from_low;
    var mapped = p.to_low;
    if span != 0.0 {
        let scaled = (value + p.pre_add) * p.multiply + p.post_add;
        mapped += (scaled - p.from_low) * ((p.to_high - p.to_low) / span);
    }
    let lo = min(p.to_low, p.to_high);
    let hi = max(p.to_low, p.to_high);
    switch p.bound {
        case 1u: { return clamp(mapped, lo, hi); }
        case 2u: {
            if hi > lo { return lo + (mapped - lo) - floor((mapped - lo) / (hi - lo)) * (hi - lo); }
        }
        case 3u: {
            // An infinite value cannot reach the range by repeated halving.
            if mapped > 0.0 && mapped <= 3.402823e38 && lo > 0.0 && hi > lo {
                while mapped > hi { mapped *= 0.5; }
                while mapped < lo { mapped *= 2.0; }
            }
        }
        default: {}
    }
    return mapped;
}

fn shade(uv: vec2f) -> vec4f {
    let color = textureSample(input, samp, uv);
    var result = color;
    if p.channels != 2u {
        result = vec4f(map_channel(color.r), map_channel(color.g), map_channel(color.b), color.a);
    }
    if p.channels != 0u { result.a = map_channel(color.a); }
    return result;
}
