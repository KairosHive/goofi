/* goofi
{ "doc": "A projected 3D Lissajous trace from HarmonicMorph.packed.\nThe first three selected components drive x, y, z. Frequencies remain continuous over a fixed exposure; the curve need not close. Phase comes from the harmonic frame. Start at 256 square; samples controls fragment cost. A high frequency or long duration needs more samples. No hidden clock drives the geometry.",
  "tags": ["image", "generator"],
  "inputs": [{"name": "harmonics", "kind": "ARRAY"}],
  "params": [
    {"group": "trace", "name": "duration", "kind": "float", "default": 4.0, "min": 0.1, "max": 16.0, "doc": "Exposure in relative-frequency seconds."},
    {"group": "trace", "name": "samples", "kind": "int", "default": 160, "min": 32, "max": 384, "doc": "Segments per pixel; increase only when curves need more detail."},
    {"group": "trace", "name": "first", "kind": "int", "default": 0, "min": 0, "max": 29, "doc": "First of three harmonic components, counting from zero."},
    {"group": "trace", "name": "decay", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0, "doc": "Extra damping along the trace."},
    {"group": "camera", "name": "yaw", "kind": "float", "default": 0.55, "min": -1000.0, "max": 1000.0, "doc": "Horizontal camera angle, radians."},
    {"group": "camera", "name": "pitch", "kind": "float", "default": 0.35, "min": -3.14, "max": 3.14, "doc": "Vertical camera angle, radians."},
    {"group": "camera", "name": "zoom", "kind": "float", "default": 0.66, "min": 0.1, "max": 2.0},
    {"group": "ink", "name": "thickness", "kind": "float", "default": 1.5, "min": 0.5, "max": 6.0, "doc": "Core line radius in pixels."},
    {"group": "ink", "name": "glow", "kind": "float", "default": 4.0, "min": 0.0, "max": 12.0},
    {"group": "common", "name": "width", "kind": "int", "default": 256, "min": 0, "max": 4096},
    {"group": "common", "name": "height", "kind": "int", "default": 256, "min": 0, "max": 4096}
  ] }
*/

fn component_row(row: i32) -> vec3f {
    return vec3f(textureLoad(harmonics, vec2i(p.first, row), 0).r,
                 textureLoad(harmonics, vec2i(p.first + 1, row), 0).r,
                 textureLoad(harmonics, vec2i(p.first + 2, row), 0).r);
}

fn project_point(t: f32, freq: vec3f, amp: vec3f, phase: vec3f, damping: vec3f) -> vec3f {
    let v = amp * sin(6.28318530718 * freq * t + phase) * exp(-damping * t);
    let cy = cos(p.yaw); let sy = sin(p.yaw);
    let cp = cos(p.pitch); let sp = sin(p.pitch);
    let turn = vec3f(cy * v.x + sy * v.z, v.y, -sy * v.x + cy * v.z);
    let q = vec3f(turn.x, cp * turn.y - sp * turn.z, sp * turn.y + cp * turn.z);
    return vec3f(q.xy * vec2f(1.0, -1.0) / max(1.0 - 0.16 * q.z, 0.3), q.z);
}

fn shade(uv: vec2f) -> vec4f {
    let dims = textureDimensions(harmonics);
    if dims.x < u32(p.first + 3) || dims.y < 4u { return vec4f(0.0); }
    let freq = component_row(0);
    let a = max(component_row(1), vec3f(0.0));
    let largest = max(a.x, max(a.y, a.z));
    if largest <= 0.0 { return vec4f(0.0); }
    let amp = a / largest;
    let phase = component_row(2);
    let damping = max(component_row(3) + p.decay, vec3f(0.0));
    let side = min(resolution.x, resolution.y);
    let here = (uv * resolution - 0.5 * resolution) / (side * 0.42 * p.zoom);
    var prev = project_point(0.0, freq, amp, phase, damping);
    var nearest = 1000.0;
    var depth = 0.0;
    var along = 0.0;
    for (var i = 1; i <= p.samples; i = i + 1) {
        let t = p.duration * f32(i) / f32(p.samples);
        let next = project_point(t, freq, amp, phase, damping);
        let edge = next.xy - prev.xy;
        let at = clamp(dot(here - prev.xy, edge) / max(dot(edge, edge), 1e-12), 0.0, 1.0);
        let d = length(here - mix(prev.xy, next.xy, at));
        if d < nearest {
            nearest = d;
            depth = mix(prev.z, next.z, at);
            along = (f32(i - 1) + at) / f32(p.samples);
        }
        prev = next;
    }
    let pixels = nearest * side * 0.42 * p.zoom;
    let core = 1.0 - smoothstep(p.thickness, p.thickness + 1.0, pixels);
    let halo = exp(-pixels * pixels / max(p.glow * p.glow, 0.01)) * 0.35;
    let color = mix(vec3f(0.15, 0.85, 0.79), vec3f(1.0, 0.63, 0.3), along);
    let brightness = 0.65 + 0.35 * clamp(0.5 + 0.35 * depth, 0.0, 1.0);
    return vec4f(color * brightness, clamp(core + halo, 0.0, 1.0));
}
