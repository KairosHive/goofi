/* goofi
{ "doc": "GPU rendering of HarmonicGeometry.geometry or GeometryBlend.geometry: curves, points, graphs and triangle meshes. Fixed orthographic view, transparent background, antialiased lines and depth-tested matte surfaces. Coordinates retain their domain: set radius to fit the geometry. Cost scales with pixels times primitives; start at 256 square. Fields use HarmonicInk instead.",
  "tags": ["image", "transform"],
  "inputs": [{"name": "geometry", "kind": "ARRAY"}],
  "params": [
    {"group": "view", "name": "radius", "kind": "float", "default": 2.0, "min": 0.01, "max": 1000.0, "doc": "Fixed coordinate half-width. No automatic fit or camera animation."},
    {"group": "ink", "name": "thickness", "kind": "float", "default": 1.5, "min": 0.5, "max": 8.0},
    {"group": "ink", "name": "style", "kind": "str", "default": "surface", "options": ["surface", "wireframe"], "doc": "Filled matte triangles or triangle edges."},
    {"group": "ink", "name": "warmth", "kind": "float", "default": 0.5, "min": 0.0, "max": 1.0},
    {"group": "common", "name": "width", "kind": "int", "default": 256, "min": 0, "max": 4096},
    {"group": "common", "name": "height", "kind": "int", "default": 256, "min": 0, "max": 4096}
  ] }
*/

fn vertex(index: u32) -> vec3f {
    let q = goofi_geo_texel(geometry, 8u+index).xyz;
    // Fixed oblique projection leaves 2D coordinates unchanged.
    return vec3f(q.x+0.35*q.z, -q.y+0.25*q.z, q.z);
}

fn part_length(index: u32, count: u32) -> u32 {
    let v = goofi_geo_texel(geometry, 8u+count+index/2u);
    return goofi_geo_uint(select(v.xy, v.zw, index % 2u == 1u));
}

fn cross2(a: vec2f, b: vec2f) -> f32 { return a.x*b.y-a.y*b.x; }

fn edge_at(q: vec2f, a: vec3f, b: vec3f) -> vec2f {
    let ab = b.xy-a.xy;
    let t = clamp(dot(q-a.xy, ab)/max(dot(ab, ab), 1e-12), 0.0, 1.0);
    return vec2f(length(q-mix(a.xy, b.xy, t)), mix(a.z, b.z, t));
}

fn shade(uv: vec2f) -> vec4f {
    if !goofi_geo_valid(geometry) { return vec4f(0.0); }
    let form = goofi_geo_header(geometry, 1u);
    if form >= 11u { return vec4f(0.0); }
    let count = goofi_geo_header(geometry, 3u);
    let parts = goofi_geo_header(geometry, 6u);
    let edges = goofi_geo_header(geometry, 7u);
    let faces = goofi_geo_header(geometry, 8u);
    let edge_start = 8u+count+(parts+1u)/2u;
    let face_start = edge_start+edges;
    var primitives = count;
    if form == 8u || form == 9u { primitives = edges; }
    if form == 10u { primitives = faces; }
    var part = 0u;
    var begin = 0u;
    var end = count;
    if parts > 0u { end = part_length(0u, count); }
    let scale = min(resolution.x, resolution.y)*0.45/p.radius;
    let q = (uv*resolution-resolution*0.5)/scale;
    var depth = -1e30;
    var result = vec4f(0.0);
    for (var i = 0u; i < primitives; i++) {
        var indices = vec3u(i);
        var kind = 1.0;
        if form == 10u {
            let first = goofi_geo_texel(geometry, face_start+i*2u);
            let second = goofi_geo_texel(geometry, face_start+i*2u+1u);
            indices = vec3u(goofi_geo_uint(first.xy), goofi_geo_uint(first.zw), goofi_geo_uint(second.xy));
            kind = 3.0;
        } else if form == 8u || form == 9u {
            let edge = goofi_geo_texel(geometry, edge_start+i);
            indices = vec3u(goofi_geo_uint(edge.xy), goofi_geo_uint(edge.zw), goofi_geo_uint(edge.zw));
            kind = 2.0;
        } else if form != 2u && form != 3u {
            while i >= end && part+1u < parts {
                begin = end;
                part++;
                end += part_length(part, count);
            }
            var next = i+1u;
            if next >= end {
                if form == 4u || form == 7u { next = begin; } else { continue; }
            }
            indices = vec3u(i, next, next);
            kind = 2.0;
        }
        if any(indices >= vec3u(count)) { continue; }
        let a = vertex(indices.x); let b = vertex(indices.y); let c = vertex(indices.z);
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
            let tone = clamp(f32(i)/f32(max(primitives-1u, 1u))*0.65+p.warmth*0.35, 0.0, 1.0);
            let color = mix(vec3f(0.10, 0.78, 0.70), vec3f(0.98, 0.70, 0.30), tone);
            result = vec4f(color*light, alpha);
            depth = near.y;
        }
    }
    return result;
}
