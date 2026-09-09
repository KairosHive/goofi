/* goofi
{ "doc": "Sculpt a signed harmonic field into a full-frame material surface.\nConnect HarmonicChladni.out to input. Zero-displacement lines become raised metal seams. Select two textures and morph their surface detail, color, and reflectance: jade, brushed metal, woven silk, or porous stone. A height-field ray march gives parallax, occlusion, and directional shadows. Mirrored field extension fills the view at any camera angle and aspect ratio. This is a stateless renderer: drive texture_mix separately from upstream mode, phase, or tuning changes. Fractional Chladni modes and material height are visual mappings, not physical plate eigenmodes. An absent or masked field shows the background.",
  "tags": ["image", "transform"],
  "state": ["reliefmap"],
  "inputs": [{"name": "input", "kind": "TEXTURE"}],
  "params": [
    {"group": "form", "name": "depth", "kind": "float", "default": 0.22, "min": 0.0, "max": 0.45, "doc": "Height of the harmonic relief. Zero removes harmonic height; texture_depth remains independent."},
    {"group": "form", "name": "seam", "kind": "float", "default": 0.022, "min": 0.008, "max": 0.15, "doc": "Width of the metal seam around zero displacement, relative to range."},
    {"group": "form", "name": "range", "kind": "float", "default": 1.0, "min": 0.01, "max": 20.0, "doc": "Fixed input field range. Sets the relief contrast without normalizing each frame."},
    {"group": "form", "name": "contours", "kind": "float", "default": 14.0, "min": 2.0, "max": 32.0, "doc": "Fine engraved levels across the signed field."},
    {"group": "form", "name": "engraving", "kind": "float", "default": 0.55, "min": 0.0, "max": 1.0, "doc": "Depth and contrast of the contour etching."},
    {"group": "camera", "name": "tilt", "kind": "float", "default": 0.25, "min": 0.0, "max": 0.85, "doc": "View tilt in radians. Higher values reveal the relief."},
    {"group": "camera", "name": "turn", "kind": "float", "default": -0.22, "min": -3.14159, "max": 3.14159, "doc": "Turn the surface in its plane, in radians."},
    {"group": "camera", "name": "zoom", "kind": "float", "default": 1.0, "min": 0.5, "max": 1.6, "doc": "Composition scale. Mirrored field extension keeps every edge filled."},
    {"group": "material", "name": "texture_a", "kind": "str", "default": "jade", "options": ["jade", "brushed metal", "woven silk", "porous stone"], "doc": "Texture at mix zero."},
    {"group": "material", "name": "texture_b", "kind": "str", "default": "brushed metal", "options": ["jade", "brushed metal", "woven silk", "porous stone"], "doc": "Texture at mix one. Choose any pair of finishes."},
    {"group": "material", "name": "texture_mix", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0, "doc": "Smoothly blend texture height, color, and reflectance. Independent of harmonic structure."},
    {"group": "material", "name": "texture_scale", "kind": "float", "default": 24.0, "min": 4.0, "max": 64.0, "doc": "Texture repeats across the source field. Detail is limited by the height-map resolution."},
    {"group": "material", "name": "texture_depth", "kind": "float", "default": 0.55, "min": 0.0, "max": 1.0, "doc": "Strength of small surface grooves, threads, and pores. Does not change the harmonic field."},
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

fn mirrored_uv(q: vec2f) -> vec2f {
    return 1.0 - abs(1.0 - 2.0*fract((q*0.5+0.5)*0.5));
}

fn texture_weights() -> vec4f {
    let indices = vec4u(0u, 1u, 2u, 3u);
    let a = select(vec4f(0.0), vec4f(1.0), indices == vec4u(min(p.texture_a, 3u)));
    let b = select(vec4f(0.0), vec4f(1.0), indices == vec4u(min(p.texture_b, 3u)));
    return mix(a, b, smoothstep(0.0, 1.0, p.texture_mix));
}

fn cell_hash(q: vec2f) -> vec2f {
    let cell = vec2u(vec2i(q) + 65536);
    var h = cell.x * 1597334673u ^ cell.y * 3812015801u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    return vec2f(f32(h & 65535u), f32(h >> 16u))/65536.0;
}

fn surface_detail(q: vec2f) -> vec4f {
    let scale = clamp(p.texture_scale, 1.0, max(1.0, min(resolution.x, resolution.y)/8.0));
    let at = (q*0.5+0.5)*scale;
    let tau = 6.28318530718;
    let jade = 0.15*sin(at.x*0.73 + sin(at.y*0.51))*sin(at.y*0.61);
    let brushed = sin(tau*(at.y + 0.045*sin(at.x*0.8)));
    let warp = cos(tau*at.x); let weft = cos(tau*at.y);
    let silk = 0.45*(warp+weft) + 0.1*warp*weft;
    var distance = 2.0;
    let cell = floor(at);
    for (var y = -1; y <= 1; y = y+1) {
        for (var x = -1; x <= 1; x = x+1) {
            let neighbor = cell + vec2f(f32(x), f32(y));
            let center = neighbor + 0.2 + 0.6*cell_hash(neighbor);
            distance = min(distance, length(at-center));
        }
    }
    let stone = 1.0 - 2.0*exp(-distance*distance*22.0);
    return vec4f(jade, brushed, silk, stone);
}

fn field_at(q: vec2f) -> vec2f {
    let value = textureSampleLevel(input, samp, mirrored_uv(q), 0.0);
    // Saturate extreme external fields before any square or contour calculation.
    let v = clamp(value.r / max(abs(p.range), 0.0001), -8.0, 8.0);
    return vec2f(v, clamp(value.a, 0.0, 1.0));
}

fn local_height(q: vec2f) -> f32 {
    let sample = field_at(q);
    if sample.y <= 0.001 { return -0.06; }
    let v = sample.x;
    let seam = max(abs(p.seam), 0.001);
    let shoulder = exp(-v*v / (32.0*seam*seam));
    let crest = exp(-v*v / (2.0*seam*seam));
    let etched = pow(0.5 + 0.5*cos(v*p.contours*6.28318530718), 12.0);
    let height = 0.035 + clamp(p.depth, 0.0, 0.45)*(0.16 + 0.35/(1.0+3.0*v*v) + 0.32*shoulder + 0.13*crest
        - 0.022*p.engraving*etched*(1.0-crest));
    let detail = dot(texture_weights(), surface_detail(q));
    return mix(-0.035, height + 0.008*clamp(p.texture_depth, 0.0, 1.0)*detail, sample.y);
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
    let value = textureSampleLevel(reliefmap, samp, mirrored_uv(at), 0.0);
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
    let top = 0.055 + clamp(p.depth, 0.0, 0.45);
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
    let field = field_at(q);
    let pixel = 2.0 / (resolution.y*zoom);
    let coverage = field.y*select(0.0, 1.0, hit);
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
        let gilding = clamp(p.metal, 0.0, 1.0)*seam;
        let weights = texture_weights();
        let detail = surface_detail(mirrored_uv(q)*2.0-1.0);
        let mineral = mix(vec3f(0.042, 0.017, 0.009), vec3f(0.004, 0.031, 0.023), p.patina);
        let finish = weights.x*mineral*(1.0+detail.x)
            + weights.y*vec3f(0.16, 0.095, 0.045)*(0.92+0.08*detail.y)
            + weights.z*vec3f(0.26, 0.22, 0.15)*(0.86+0.14*detail.z)
            + weights.w*vec3f(0.065, 0.077, 0.064)*(0.8+0.2*detail.w);
        let basin = finish * mix(0.5, 1.4, smoothstep(-0.6, 0.7, v));
        let gold = vec3f(0.48, 0.31, 0.12);
        let albedo = mix(basin, gold, gilding);
        let etched = pow(0.5+0.5*cos(v*p.contours*6.28318530718), 16.0)*(1.0-seam);
        let occlusion = 0.6 + 0.4/(1.0+length(slope)*0.7);
        let diffuse = max(dot(normal, light), 0.0);
        let finish_roughness = dot(weights, vec4f(0.0, 0.1, 0.22, 0.38));
        let rough = pow(clamp(p.roughness + finish_roughness*(1.0-gilding), 0.12, 0.9), 2.0);
        let nh = max(dot(normal, halfway), 0.0);
        let distribution = rough*rough / (3.14159265359*pow(nh*nh*(rough*rough-1.0)+1.0, 2.0));
        let reflectance = mix(mix(vec3f(0.045), vec3f(0.42, 0.27, 0.13), weights.y), gold, gilding);
        let fresnel = reflectance + (1.0-reflectance)*pow(1.0-max(dot(halfway, view), 0.0), 5.0);
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
