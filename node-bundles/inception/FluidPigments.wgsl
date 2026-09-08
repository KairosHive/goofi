/* goofi
{ "doc": "Sixteen EEG features drive liquid pigments, morphing and color.\nInputs 1-8: normalized LZ; 9-16: normalized Higuchi dimension.",
  "tags": ["image", "simulation"],
  "inputs": [{"name": "features", "kind": "ARRAY"}],
  "state": ["dye"],
  "params": [
    {"group": "fluid", "name": "speed", "kind": "float", "default": 0.7, "min": 0.0, "max": 2.0},
    {"group": "fluid", "name": "swirl", "kind": "float", "default": 1.0, "min": 0.0, "max": 3.0},
    {"group": "fluid", "name": "color", "kind": "float", "default": 1.0, "min": 0.0, "max": 2.0},
    {"group": "fluid", "name": "relief", "kind": "float", "default": 0.65, "min": 0.0, "max": 2.0},
    {"group": "fluid", "name": "light", "kind": "float", "default": 1.15, "min": 0.1, "max": 3.0},
    {"group": "fluid", "name": "flow_scale", "kind": "float", "default": 1.0, "min": 0.3, "max": 3.0},
    {"group": "fluid", "name": "eddies", "kind": "float", "default": 1.0, "min": 0.0, "max": 3.0},
    {"group": "fluid", "name": "stretch", "kind": "float", "default": 0.3, "min": 0.0, "max": 2.0},
    {"group": "fluid", "name": "morph", "kind": "float", "default": 0.65, "min": 0.0, "max": 2.0},
    {"group": "fluid", "name": "renewal", "kind": "float", "default": 0.006, "min": 0.0, "max": 0.04},
    {"group": "fluid", "name": "detail", "kind": "float", "default": 1.0, "min": 0.2, "max": 3.0},
    {"group": "fluid", "name": "palette_shift", "kind": "float", "default": 0.0, "min": -1.0, "max": 1.0},
    {"group": "fluid", "name": "color_spread", "kind": "float", "default": 1.0, "min": 0.2, "max": 2.0},
    {"group": "fluid", "name": "saturation", "kind": "float", "default": 1.0, "min": 0.0, "max": 2.0},
    {"group": "fluid", "name": "refraction", "kind": "float", "default": 1.0, "min": 0.0, "max": 3.0},
    {"group": "fluid", "name": "gloss", "kind": "float", "default": 1.0, "min": 0.0, "max": 2.0},
    {"group": "fluid", "name": "roughness", "kind": "float", "default": 0.35, "min": 0.05, "max": 1.0},
    {"group": "fluid", "name": "light_angle", "kind": "float", "default": 0.0, "min": -3.14, "max": 3.14},
    {"group": "fluid", "name": "contrast", "kind": "float", "default": 1.0, "min": 0.4, "max": 2.0},
    {"group": "fluid", "name": "eeg_motion", "kind": "float", "default": 1.0, "min": 0.0, "max": 3.0},
    {"group": "fluid", "name": "eeg_color", "kind": "float", "default": 0.35, "min": 0.0, "max": 2.0} ] }
*/
fn eeg(i: i32) -> f32 {
    let d = vec2i(textureDimensions(features));
    if d.x * d.y < 16 {
        return 0.5;
    }
    let j = i % 16;
    return clamp(textureLoad(features, vec2i(j % d.x, j / d.x), 0).r, 0.0, 1.0);
}

fn hash(v: vec2f) -> f32 {
    let q = vec2u(vec2i(v) + 65536);
    var h = q.x * 1597334673u ^ q.y * 3812015801u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h ^ (h >> 13u)) / 4294967296.0;
}

fn noise(v: vec2f) -> f32 {
    let a = floor(v);
    let f = fract(v);
    let w = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hash(a), hash(a + vec2f(1, 0)), w.x),
        mix(hash(a + vec2f(0, 1)), hash(a + vec2f(1, 1)), w.x),
        w.y
    );
}

fn fbm(v: vec2f) -> f32 {
    var q = v;
    var s = 0.0;
    var a = 0.55;
    for (var i = 0; i < 5; i++) {
        s += a * noise(q);
        q = vec2f(q.x * 1.63 - q.y * 1.17, q.x * 1.17 + q.y * 1.63) + 3.7;
        a *= 0.48;
    }
    return s;
}

fn seed(uv: vec2f) -> vec4f {
    let drive = (eeg(0) + eeg(3) + eeg(8) + eeg(11)) * 0.25 - 0.5;
    let q = uv * vec2f(4.8, 3.6) * (1.0 + 0.3 * p.morph * drive) + p.morph * vec2f(eeg(2) - 0.5, eeg(10) - 0.5);
    let w = vec2f(fbm(q + 7.1), fbm(q + 31.8));
    let n = fbm(q + 3.6 * w);
    let m = fbm(q * 1.3 + 4.2 * w + 19.0);
    let ribbons = 0.5 + 0.5 * sin((18.0 * n + 9.0 * m) * p.detail + q.x * 1.8);
    return vec4f(n, m, ribbons, 1.0);
}

fn flow(uv: vec2f) -> vec2f {
    var v = vec2f(0.0);
    let aspect = vec2f(resolution.x / resolution.y, 1.0);
    let t = time * 0.08;
    for (var i = 0; i < 8; i++) {
        let k = i;
        let fi = f32(i);
        let centre = vec2f(
            0.5 + 0.39 * sin(fi * 2.399 + (eeg(k) - 0.5) * 2.8 * p.eeg_motion + t * 0.17),
            0.5 + 0.39 * cos(fi * 1.713 + (eeg(k + 8) - 0.5) * 2.8 * p.eeg_motion - t * 0.13)
        );
        let d = (uv - centre) * aspect * p.flow_scale;
        let r2 = dot(d, d);
        let strength = (0.7 + (eeg(k) - 0.5) * 1.3 * p.eeg_motion) * select(-1.0, 1.0, i % 2 == 0);
        let width = 0.015 + 0.12 * eeg(k + 8);
        v += vec2f(-d.y, d.x) * strength * exp(-r2 / width) * (0.65 + eeg(k + 4));
        // A stream-function wave contributes a second scale of curl.
        let f = (4.0 + eeg(k + 8) * 18.0) * p.flow_scale;
        let phase = fi + eeg(k + 6) * 3.0 + t * (0.2 + eeg(k + 7));
        v += 0.006 * p.eddies * vec2f(
            f * sin(uv.x * f + phase) * cos(uv.y * f - phase),
            -f * cos(uv.x * f + phase) * sin(uv.y * f - phase)
        );
    }
    v += p.stretch * 0.12 * vec2f(sin(uv.y * 7.0 + eeg(5) * 3.0), cos(uv.x * 6.0 + eeg(13) * 3.0));
    return v / aspect;
}

fn readDye(uv: vec2f) -> vec4f {
    return textureSampleLevel(dye, samp, clamp(uv, vec2f(0.001), vec2f(0.999)), 0.0);
}

fn next_dye(uv: vec2f) -> vec4f {
    if frame == 0u {
        return seed(uv);
    }
    let dt = 0.006 * p.speed;
    let v = flow(uv) * p.swirl;
    let mid = uv - v * dt * 0.5;
    let back = uv - flow(mid) * p.swirl * dt;
    let old = readDye(back);
    // Slow replenishment keeps fine marbling alive instead of diffusing to a flat color.
    let fresh = seed(uv + 0.035 * vec2f(sin(time * 0.023), cos(time * 0.019)));
    return vec4f(mix(old.rgb, fresh.rgb, p.renewal), 1.0);
}

fn palette(x: f32) -> vec3f {
    let t = fract(x) * 6.0;
    let a = vec3f(0.018, 0.055, 0.105);
    let b = vec3f(0.018, 0.32, 0.34);
    let c = vec3f(0.35, 0.57, 0.39);
    let d = vec3f(0.92, 0.67, 0.28);
    let e = vec3f(0.67, 0.19, 0.12);
    let f = vec3f(0.26, 0.075, 0.32);
    if t < 1.0 {
        return mix(a, b, smoothstep(0.0, 1.0, t));
    }
    if t < 2.0 {
        return mix(b, c, smoothstep(1.0, 2.0, t));
    }
    if t < 3.0 {
        return mix(c, d, smoothstep(2.0, 3.0, t));
    }
    if t < 4.0 {
        return mix(d, e, smoothstep(3.0, 4.0, t));
    }
    if t < 5.0 {
        return mix(e, f, smoothstep(4.0, 5.0, t));
    }
    return mix(f, a, smoothstep(5.0, 6.0, t));
}

fn heightAt(uv: vec2f) -> f32 {
    let d = readDye(uv);
    return d.x * 0.6 + d.y * 0.25 + 0.05 * sin(d.z * 10.0 + d.x * 17.0);
}

fn shade(uv: vec2f) -> vec4f {
    let d = readDye(uv);
    let px = 2.0 / resolution;
    let h = heightAt(uv);
    let grad = vec2f(
        heightAt(uv + vec2f(px.x, 0)) - heightAt(uv - vec2f(px.x, 0)),
        heightAt(uv + vec2f(0, px.y)) - heightAt(uv - vec2f(0, px.y))
    ) / px;
    let n = normalize(vec3f(-grad * p.relief * 0.23, 1.0));
    let view = normalize(vec3f((uv - 0.5) * 0.35, 1.0));
    let l = normalize(vec3f(
        -0.45 * cos(p.light_angle) - 0.55 * sin(p.light_angle),
        -0.45 * sin(p.light_angle) + 0.55 * cos(p.light_angle),
        0.9
    ));
    let key = 0.38 + 0.62 * max(dot(n, l), 0.0);
    let spec = pow(max(dot(n, normalize(l + view)), 0.0), mix(180.0, 8.0, p.roughness));
    let soft = pow(max(dot(n, normalize(vec3f(0.7, -0.3, 1.0))), 0.0), 12.0);
    let shift = readDye(uv + n.xy * 0.012 * p.refraction);
    let bands = sin(shift.x * 38.0 + shift.y * 24.0 + shift.z * 4.0);
    let pigment = d.x * 1.7 + d.y * 0.85 + d.z * 0.2 + 0.08 * bands;
    let local = sin(uv.x * 4.0 + eeg(6) * 2.0) * (eeg(14) - 0.5) + cos(uv.y * 5.0 + eeg(7) * 2.0) * (eeg(15) - 0.5);
    let hue = p.palette_shift + p.eeg_color * ((eeg(1) + eeg(9) - 1.0) * 0.35 + local * 0.18);
    var c = palette(((pigment - 1.0) * p.color_spread + 1.0) * p.color + hue);
    c = max(mix(vec3f(dot(c, vec3f(0.2126, 0.7152, 0.0722))), c, p.saturation), vec3f(0.0));
    let fine = 0.5 + 0.5 * sin(d.x * 125.0 + d.y * 61.0);
    c *= key * (0.82 + 0.18 * fine);
    let valley = clamp(0.6 + (h - 0.3) * 1.2, 0.45, 1.0);
    c *= valley;
    c += p.light * (vec3f(1.0, 0.91, 0.72) * spec * 0.48 * p.gloss + vec3f(0.25, 0.45, 0.54) * soft * 0.12 * p.gloss);
    c *= 1.0 - 0.24 * dot(uv - 0.5, uv - 0.5);
    c = pow(max(c, vec3f(0.0)), vec3f(p.contrast));
    c = vec3f(1.0) - exp(-c * 1.9);
    return vec4f(pow(max(c, vec3f(0.0)), vec3f(0.9)), 1.0);
}
