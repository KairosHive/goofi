/* goofi
{ "doc": "Sculpt a signed harmonic field into jade and metal relief.\nConnect HarmonicChladni.out to input. Zero-displacement lines become raised metal seams; signed basins become glazed mineral, with fine contour engraving. A height-field ray march gives parallax, occlusion, a beveled plate edge, and directional shadows. The material reads the field without changing it. This is a stateless renderer: motion comes from upstream mode, phase, or tuning changes. Fractional Chladni modes remain visual transitions, not physical plate eigenmodes. An absent or masked field shows only the background.",
  "tags": ["image", "transform"],
  "state": ["reliefmap"],
  "inputs": [{"name": "input", "kind": "TEXTURE"}],
  "params": [
    {"group": "form", "name": "depth", "kind": "float", "default": 0.22, "min": 0.0, "max": 0.45, "doc": "Height of the harmonic relief. Zero keeps a flat engraved plate."},
    {"group": "form", "name": "seam", "kind": "float", "default": 0.022, "min": 0.008, "max": 0.15, "doc": "Width of the metal seam around zero displacement, relative to range."},
    {"group": "form", "name": "range", "kind": "float", "default": 1.0, "min": 0.01, "max": 20.0, "doc": "Fixed input field range. Sets the relief contrast without normalizing each frame."},
    {"group": "form", "name": "contours", "kind": "float", "default": 14.0, "min": 2.0, "max": 32.0, "doc": "Fine engraved levels across the signed field."},
    {"group": "form", "name": "engraving", "kind": "float", "default": 0.55, "min": 0.0, "max": 1.0, "doc": "Depth and contrast of the contour etching."},
    {"group": "camera", "name": "tilt", "kind": "float", "default": 0.42, "min": 0.0, "max": 0.85, "doc": "View tilt in radians. Higher values reveal relief and edge thickness."},
    {"group": "camera", "name": "turn", "kind": "float", "default": -0.22, "min": -3.14159, "max": 3.14159, "doc": "Turn the whole plate in its plane, in radians."},
    {"group": "camera", "name": "zoom", "kind": "float", "default": 0.87, "min": 0.5, "max": 1.6, "doc": "Composition scale. Values above one crop into the relief."},
    {"group": "camera", "name": "roundness", "kind": "float", "default": 0.3, "min": 0.0, "max": 1.0, "doc": "Rounded square to circular plate. This crops the picture, not the upstream boundary modes."},
    {"group": "material", "name": "patina", "kind": "float", "default": 0.7, "min": 0.0, "max": 1.0, "doc": "Smoked bronze to deep jade in the glazed basins."},
    {"group": "material", "name": "metal", "kind": "float", "default": 0.8, "min": 0.0, "max": 1.0, "doc": "Champagne metal on nodal seams. Zero keeps mineral ridges."},
    {"group": "material", "name": "roughness", "kind": "float", "default": 0.38, "min": 0.12, "max": 0.8, "doc": "Low values give tight polished highlights. High values give broad satin highlights."},
    {"group": "light", "name": "azimuth", "kind": "float", "default": -0.75, "min": -3.14159, "max": 3.14159, "doc": "Direction of the key light around the plate, in radians."},
    {"group": "light", "name": "elevation", "kind": "float", "default": 0.85, "min": 0.3, "max": 1.5, "doc": "Light height in radians. Low light shows long relief shadows."},
    {"group": "light", "name": "exposure", "kind": "float", "default": 1.5, "min": 0.3, "max": 3.0, "doc": "Filmic exposure before display gamma."},
    {"group": "common", "name": "width", "kind": "int", "default": 512, "min": 0, "max": 4096},
    {"group": "common", "name": "height", "kind": "int", "default": 512, "min": 0, "max": 4096}
  ] }
*/

fn local_point(q: vec2f) -> vec2f {
    let c = cos(p.turn); let s = sin(p.turn);
    return vec2f(c*q.x + s*q.y, -s*q.x + c*q.y);
}

fn edge_distance(q: vec2f) -> f32 {
    let power = mix(8.0, 2.0, clamp(p.roundness, 0.0, 1.0));
    let a = abs(q);
    return 1.0 - pow(pow(a.x, power) + pow(a.y, power), 1.0 / power);
}

fn field_at(q: vec2f) -> vec2f {
    let value = textureSampleLevel(input, samp, clamp(q*0.5 + 0.5, vec2f(0.0), vec2f(1.0)), 0.0);
    // Saturate extreme external fields before any square or contour calculation.
    let v = clamp(value.r / max(abs(p.range), 0.0001), -8.0, 8.0);
    return vec2f(v, clamp(value.a, 0.0, 1.0));
}

fn local_height(q: vec2f) -> f32 {
    let edge = edge_distance(q);
    if edge <= 0.0 { return -0.06; }
    let sample = field_at(q);
    if sample.y <= 0.001 { return -0.06; }
    let v = sample.x;
    let seam = max(abs(p.seam), 0.001);
    let shoulder = exp(-v*v / (32.0*seam*seam));
    let crest = exp(-v*v / (2.0*seam*seam));
    let etched = pow(0.5 + 0.5*cos(v*p.contours*6.28318530718), 12.0);
    let height = 0.035 + clamp(p.depth, 0.0, 0.45)*(0.16 + 0.35/(1.0+3.0*v*v) + 0.32*shoulder + 0.13*crest
        - 0.022*p.engraving*etched*(1.0-crest));
    return mix(-0.035, height, smoothstep(0.0, 0.045, edge)*sample.y);
}

fn next_reliefmap(uv: vec2f) -> vec4f {
    // Rebuild from the input every frame. This render pass never reads its history.
    let height = local_height(uv*2.0-1.0);
    // The engine's targets use float16. Retain the residual for smooth relief normals.
    let high = floor(height*2048.0)/2048.0;
    return vec4f(high, (height-high)*2048.0, 0.0, 1.0);
}

fn relief(q: vec2f) -> f32 {
    let at = local_point(q);
    if any(abs(at) >= vec2f(1.0)) { return -0.06; }
    let value = textureSampleLevel(reliefmap, samp, at*0.5+0.5, 0.0);
    return value.r+value.g/2048.0;
}

fn grain(q: vec2f) -> f32 {
    let cell = vec2u(vec2i(floor(q*900.0)) + 65536);
    var h = cell.x * 1597334673u ^ cell.y * 3812015801u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    return f32(h ^ (h >> 13u)) / 4294967296.0;
}

fn shade(uv: vec2f) -> vec4f {
    let zoom = max(abs(p.zoom), 0.1);
    let screen = (uv*2.0-1.0) * vec2f(resolution.x/resolution.y, -1.0) / zoom;
    let tilt = clamp(p.tilt, 0.0, 0.85);
    let st = sin(tilt); let ct = cos(tilt);
    let eye = vec3f(screen.x, screen.y*ct + 2.0*st, 2.0*ct - screen.y*st);
    let ray = vec3f(0.0, -st, -ct);
    let top = 0.04 + clamp(p.depth, 0.0, 0.45);
    let start = (top-eye.z)/ray.z;
    let end = (-0.06-eye.z)/ray.z;
    let step = (end-start)/40.0;
    var far = start; var near = start;
    var hit = false;
    // Fixed work budget: forty steps and eight sub-step refinements.
    for (var i = 0; i <= 40; i = i+1) {
        far = start + f32(i)*step;
        let at = eye + ray*far;
        if at.z <= relief(at.xy) { hit = true; break; }
        near = far;
    }
    for (var i = 0; i < 8; i = i+1) {
        if hit {
            let mid = 0.5*(near+far); let at = eye+ray*mid;
            if at.z > relief(at.xy) { near = mid; } else { far = mid; }
        }
    }
    let position = eye+ray*far;
    let q = local_point(position.xy);
    let edge = edge_distance(q);
    let field = field_at(q);
    let pixel = 2.0 / (resolution.y*zoom);
    let coverage = smoothstep(0.0, pixel*1.5, edge)*field.y*select(0.0, 1.0, hit);
    let backdrop = mix(vec3f(0.009, 0.018, 0.022), vec3f(0.020, 0.032, 0.037),
        exp(-0.5*dot(screen, screen)));
    var color = backdrop;
    if coverage > 0.0 {
        let epsilon = max(0.0015, pixel*0.7);
        let dx = vec2f(epsilon, 0.0); let dy = dx.yx;
        let slope = vec2f(relief(position.xy+dx)-relief(position.xy-dx),
                          relief(position.xy+dy)-relief(position.xy-dy)) / (2.0*epsilon);
        let normal = normalize(vec3f(-slope, 1.0));
        let light = vec3f(cos(p.azimuth)*cos(p.elevation), sin(p.azimuth)*cos(p.elevation), sin(p.elevation));
        let view = -ray; let halfway = normalize(light+view);
        var shadow = 1.0;
        for (var i = 1; i <= 10; i = i+1) {
            let distance = 0.012 + 0.018*f32(i*i);
            let at = position + light*distance;
            shadow = min(shadow, smoothstep(-0.006, 0.018+0.025*distance, at.z-relief(at.xy)));
        }
        let v = field.x;
        let seam_width = max(abs(p.seam), 0.001);
        let seam = exp(-v*v/(2.0*seam_width*seam_width));
        let lip = exp(-pow((edge-0.018)/0.007, 2.0));
        let gilding = p.metal*max(seam, lip*0.8);
        let mineral = mix(vec3f(0.042, 0.017, 0.009), vec3f(0.004, 0.031, 0.023), p.patina);
        let basin = mineral * mix(0.5, 1.4, smoothstep(-0.6, 0.7, v));
        let gold = vec3f(0.48, 0.31, 0.12);
        let albedo = mix(basin, gold, gilding);
        let etched = pow(0.5+0.5*cos(v*p.contours*6.28318530718), 16.0)*(1.0-seam);
        let occlusion = 0.6 + 0.4/(1.0+length(slope)*0.7);
        let diffuse = max(dot(normal, light), 0.0);
        let rough = pow(clamp(p.roughness, 0.12, 0.8), 2.0);
        let nh = max(dot(normal, halfway), 0.0);
        let distribution = rough*rough / (3.14159265359*pow(nh*nh*(rough*rough-1.0)+1.0, 2.0));
        let fresnel = mix(vec3f(0.045), gold, gilding)
            + (1.0-mix(vec3f(0.045), gold, gilding))*pow(1.0-max(dot(halfway, view), 0.0), 5.0);
        let specular = fresnel*distribution*diffuse/(0.35+4.0*max(dot(normal, view), 0.0));
        let sky = pow(max(dot(normal, normalize(vec3f(-0.4, 0.8, 0.65))), 0.0), 3.0);
        let rim = pow(1.0-max(dot(normal, view), 0.0), 3.0);
        color = albedo*(0.45*occlusion + 2.7*diffuse*mix(0.25, 1.0, shadow));
        color += specular*shadow*2.0 + vec3f(0.025, 0.075, 0.070)*sky*(0.3+0.7*gilding);
        color += vec3f(0.16, 0.22, 0.19)*rim*occlusion;
        color *= 1.0 - p.engraving*etched*0.42;
        color *= 0.97+0.06*grain(q);
        color = mix(backdrop, color, coverage);
    }
    color *= 1.0 - 0.14*smoothstep(0.3, 2.0, dot(uv-0.5, uv-0.5)*4.0);
    let mapped = 1.0-exp(-max(color, vec3f(0.0))*max(p.exposure, 0.0));
    return vec4f(pow(mapped, vec3f(1.0/2.2)), 1.0);
}
