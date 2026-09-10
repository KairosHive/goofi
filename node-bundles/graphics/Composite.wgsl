/* goofi
{ "doc": "two textures into one\nA is the foreground and B is the background. Under puts B over A. Subtract is B - A; divide is B / A (zero divisors give zero). Minimum and maximum compare each RGB channel. In keeps A inside B's alpha; out keeps A outside it; atop keeps B's alpha; xor keeps non-overlapping parts. Color modes blend where the inputs overlap and use source-over alpha. RGB is not clamped, except in color dodge/burn. Blend fades the result back to B with alpha-correct interpolation.",
  "tags": ["image", "transform"],
  "inputs": [{"name": "a", "kind": "TEXTURE"}, {"name": "b", "kind": "TEXTURE"}],
  "params": [
    {"group": "composite", "name": "mode", "kind": "str", "default": "over",
     "options": ["over", "under", "add", "subtract", "multiply", "divide", "minimum", "maximum", "screen", "overlay", "hard light", "soft light", "color dodge", "color burn", "difference", "exclusion", "in", "out", "atop", "xor"]},
    {"group": "composite", "name": "blend", "kind": "float", "default": 1.0, "min": 0.0, "max": 1.0} ] }
*/
fn blend_channel(source: f32, backdrop: f32) -> f32 {
    switch p.mode {
        case 2u: { return backdrop + source; }
        case 3u: { return backdrop - source; }
        case 4u: { return backdrop * source; }
        case 5u: {
            if source == 0.0 { return 0.0; }
            return backdrop / source;
        }
        case 6u: { return min(backdrop, source); }
        case 7u: { return max(backdrop, source); }
        case 8u: { return backdrop + source - backdrop * source; }
        case 9u, 10u: {
            let threshold = select(source, backdrop, p.mode == 9u);
            if threshold <= 0.5 { return 2.0 * backdrop * source; }
            return 1.0 - 2.0 * (1.0 - backdrop) * (1.0 - source);
        }
        case 11u: {
            if source <= 0.5 {
                return backdrop - (1.0 - 2.0 * source) * backdrop * (1.0 - backdrop);
            }
            var d = sqrt(max(backdrop, 0.0));
            if backdrop <= 0.25 { d = ((16.0 * backdrop - 12.0) * backdrop + 4.0) * backdrop; }
            return backdrop + (2.0 * source - 1.0) * (d - backdrop);
        }
        case 12u: {
            if backdrop <= 0.0 { return 0.0; }
            if source >= 1.0 { return 1.0; }
            return clamp(backdrop / (1.0 - source), 0.0, 1.0);
        }
        case 13u: {
            if backdrop >= 1.0 { return 1.0; }
            if source <= 0.0 { return 0.0; }
            return 1.0 - clamp((1.0 - backdrop) / source, 0.0, 1.0);
        }
        case 14u: { return abs(backdrop - source); }
        case 15u: { return backdrop + source - 2.0 * backdrop * source; }
        default: { return source; }
    }
}

fn shade(uv: vec2f) -> vec4f {
    let ca = textureSample(a, samp, uv);
    let cb = textureSample(b, samp, uv);
    let aa = clamp(ca.a, 0.0, 1.0);
    let ab = clamp(cb.a, 0.0, 1.0);
    var fa = 1.0;
    var fb = 1.0 - aa;
    switch p.mode {
        case 1u: { fa = 1.0 - ab; fb = 1.0; }
        case 16u: { fa = ab; fb = 0.0; }
        case 17u: { fa = 1.0 - ab; fb = 0.0; }
        case 18u: { fa = ab; }
        case 19u: { fa = 1.0 - ab; }
        default: {}
    }
    var source = ca.rgb;
    if p.mode >= 2u && p.mode <= 15u {
        let blended = vec3f(blend_channel(ca.r, cb.r), blend_channel(ca.g, cb.g), blend_channel(ca.b, cb.b));
        source = mix(ca.rgb, blended, ab);
    }
    let alpha = mix(ab, aa * fa + ab * fb, p.blend);
    let premultiplied = mix(cb.rgb * ab, source * aa * fa + cb.rgb * ab * fb, p.blend);
    if p.blend == 0.0 { return cb; }
    if alpha <= 0.0 { return vec4f(0.0); }
    return vec4f(premultiplied / alpha, alpha);
}
