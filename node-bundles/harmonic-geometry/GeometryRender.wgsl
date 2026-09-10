/* goofi
{ "doc": "GPU rendering of GeometryUpload.primitives: curves, points, graphs and triangle meshes. Fixed orthographic view, transparent background, antialiased lines and depth-tested matte surfaces. Coordinates retain their domain: set radius to fit the geometry. Cost scales with pixels times primitives; start at 256 square. Fields use HarmonicInk instead.",
  "tags": ["image", "transform"],
  "inputs": [{"name": "primitives", "kind": "ARRAY"}],
  "params": [
    {"group": "view", "name": "radius", "kind": "float", "default": 2.0, "min": 0.01, "max": 1000.0, "doc": "Fixed coordinate half-width. No automatic fit or camera animation."},
    {"group": "ink", "name": "thickness", "kind": "float", "default": 1.5, "min": 0.5, "max": 8.0},
    {"group": "ink", "name": "style", "kind": "str", "default": "surface", "options": ["surface", "wireframe"], "doc": "Filled matte triangles or triangle edges."},
    {"group": "ink", "name": "warmth", "kind": "float", "default": 0.5, "min": 0.0, "max": 1.0},
    {"group": "common", "name": "width", "kind": "int", "default": 256, "min": 0, "max": 4096},
    {"group": "common", "name": "height", "kind": "int", "default": 256, "min": 0, "max": 4096}
  ] }
*/

fn value(row: i32, col: i32) -> f32 {
    return textureLoad(primitives, vec2i(col, row), 0).r;
}

fn vertex(row: i32, col: i32) -> vec3f {
    let q = vec3f(value(row, col), value(row, col+1), value(row, col+2));
    // Fixed oblique depth projection leaves all 2D coordinates unchanged.
    return vec3f(q.x + 0.35*q.z, -q.y + 0.25*q.z, q.z);
}

fn cross2(a: vec2f, b: vec2f) -> f32 { return a.x*b.y-a.y*b.x; }

fn edge_at(q: vec2f, a: vec3f, b: vec3f) -> vec2f {
    let ab = b.xy-a.xy;
    let t = clamp(dot(q-a.xy, ab)/max(dot(ab, ab), 1e-12), 0.0, 1.0);
    return vec2f(length(q-mix(a.xy, b.xy, t)), mix(a.z, b.z, t));
}

fn shade(uv: vec2f) -> vec4f {
    let dims = textureDimensions(primitives);
    if dims.x != 12u || dims.y > 4096u { return vec4f(0.0); }
    let scale = min(resolution.x, resolution.y)*0.45/p.radius;
    let q = (uv*resolution-resolution*0.5)/scale;
    var depth = -1e30;
    var result = vec4f(0.0);
    for (var i = 0; i < i32(dims.y); i = i+1) {
        let kind = value(i, 9);
        if kind < 1.0 { continue; }
        let a = vertex(i, 0); let b = vertex(i, 3); let c = vertex(i, 6);
        var near = edge_at(q, a, b);
        var light = 1.0;
        var fill = false;
        if kind == 3.0 {
            let bc = edge_at(q, b, c); let ca = edge_at(q, c, a);
            if bc.x < near.x { near = bc; }
            if ca.x < near.x { near = ca; }
            let area = cross2(b.xy-a.xy, c.xy-a.xy);
            if abs(area) > 1e-10 {
                let v = cross2(q-a.xy, c.xy-a.xy)/area;
                let w = cross2(b.xy-a.xy, q-a.xy)/area;
                if p.style == 0u && v >= 0.0 && w >= 0.0 && v+w <= 1.0 {
                    fill = true;
                    near.y = a.z*(1.0-v-w)+b.z*v+c.z*w;
                }
                let normal = normalize(cross(b-a, c-a));
                light = 0.35+0.65*abs(dot(normal, normalize(vec3f(0.3, -0.5, 0.8))));
            }
        }
        let alpha = select(1.0-smoothstep(p.thickness, p.thickness+1.0, near.x*scale), 1.0, fill);
        if alpha > 0.001 && near.y >= depth {
            let tone = clamp(value(i, 10)*0.65+p.warmth*0.35, 0.0, 1.0);
            let color = mix(vec3f(0.10, 0.78, 0.70), vec3f(0.98, 0.70, 0.30), tone);
            result = vec4f(color*light, alpha);
            depth = near.y;
        }
    }
    return result;
}
