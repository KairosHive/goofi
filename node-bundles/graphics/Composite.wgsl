/* goofi
{ "doc": "two textures into one\nA is the foreground and B is the background. Under puts B over A. Subtract is B - A; divide is B / A (zero divisors give zero). Minimum and maximum compare each RGB channel. In keeps A inside B's alpha; out keeps A outside it; atop keeps B's alpha; xor keeps non-overlapping parts. Color modes blend where the inputs overlap and use source-over alpha. RGB is not clamped, except in color dodge/burn and the four last modes. Hue, saturation, color and luminosity are the W3C non-separable modes: color keeps B's light and wears A's hue and saturation, luminosity keeps B's hue and saturation and takes A's light. Blend fades the result back to B with alpha-correct interpolation. A wired mask scales blend texel by texel, by its luminance times its alpha, so the result is B wherever the mask is black or clear.",
  "tags": ["image", "transform"],
  "inputs": [{"name": "a", "kind": "TEXTURE"}, {"name": "b", "kind": "TEXTURE"}, {"name": "mask", "kind": "TEXTURE"}],
  "params": [
    {"group": "composite", "name": "mode", "kind": "str", "default": "over",
     "options": ["over", "under", "add", "subtract", "multiply", "divide", "minimum", "maximum", "screen", "overlay", "hard light", "soft light", "color dodge", "color burn", "difference", "exclusion", "in", "out", "atop", "xor", "hue", "saturation", "color", "luminosity"]},
    {"group": "composite", "name": "blend", "kind": "num", "default": 1.0, "min": 0.0, "max": 1.0} ] }
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

// The W3C compositing spec's non-separable modes, on light by its 0.3 / 0.59 / 0.11 weights.
fn light_of(c: vec3f) -> f32 {
    return dot(c, vec3f(0.3, 0.59, 0.11));
}

fn into_gamut(c: vec3f) -> vec3f {
    let l = light_of(c);
    let n = min(c.r, min(c.g, c.b));
    let x = max(c.r, max(c.g, c.b));
    var out = c;
    if n < 0.0 { out = l + (out - l) * l / max(l - n, 1e-6); }
    if x > 1.0 { out = l + (out - l) * (1.0 - l) / max(x - l, 1e-6); }
    return out;
}

fn with_light(c: vec3f, l: f32) -> vec3f {
    return into_gamut(c + (l - light_of(c)));
}

fn saturation_of(c: vec3f) -> f32 {
    return max(c.r, max(c.g, c.b)) - min(c.r, min(c.g, c.b));
}

fn with_saturation(c: vec3f, s: f32) -> vec3f {
    let n = min(c.r, min(c.g, c.b));
    let x = max(c.r, max(c.g, c.b));
    if x <= n { return vec3f(0.0); }
    return (c - n) * s / (x - n);
}

fn blend_whole(source: vec3f, backdrop: vec3f) -> vec3f {
    switch p.mode {
        case 20u: { return with_light(with_saturation(source, saturation_of(backdrop)), light_of(backdrop)); }
        case 21u: { return with_light(with_saturation(backdrop, saturation_of(source)), light_of(backdrop)); }
        case 22u: { return with_light(source, light_of(backdrop)); }
        default: { return with_light(backdrop, light_of(source)); }
    }
}

fn shade(uv: vec2f) -> vec4f {
    let ca = textureSample(a, samp, uv);
    let cb = textureSample(b, samp, uv);
    // An unwired input is a 1x1 clear texture, so a mask counts only when something is wired.
    let m = textureSample(mask, samp, uv);
    let size = textureDimensions(mask);
    let wired = size.x > 1u || size.y > 1u;
    let k = select(p.blend, p.blend * clamp(dot(m.rgb, vec3f(0.299, 0.587, 0.114)) * m.a, 0.0, 1.0), wired);
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
    } else if p.mode >= 20u {
        source = mix(ca.rgb, blend_whole(ca.rgb, cb.rgb), ab);
    }
    let alpha = mix(ab, aa * fa + ab * fb, k);
    let premultiplied = mix(cb.rgb * ab, source * aa * fa + cb.rgb * ab * fb, k);
    if k == 0.0 { return cb; }
    if alpha <= 0.0 { return vec4f(0.0); }
    return vec4f(premultiplied / alpha, alpha);
}
