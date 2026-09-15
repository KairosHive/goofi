/* goofi
{ "doc": "Accepts a signed input texture or a canonical field ARRAY on geometry; a valid geometry input takes precedence. Draw a harmonic field as grains, living textures, or sculpted material.\nConnect HarmonicChladni.out to input. Choose any two textures and morph their surface detail, density, color, and light response. Sand, dunes, lichen, coral, cells, spores, pollen, and plankton join jade, brushed metal, woven silk, and porous stone. Organic textures follow the field with small rough features and restrained highlights. A height-field ray march gives parallax and shadows. Mirrored extension fills the view. This is a stateless visual mapping: grains and cells are procedural features, not tracked particles or a biological simulation. Drive texture_mix separately from upstream mode, phase, or tuning changes. An absent or masked field shows the background.",
  "tags": ["image", "transform"],
  "state": ["reliefmap"],
  "inputs": [{"name": "input", "kind": "TEXTURE"}, {"name": "geometry", "kind": "ARRAY"}],
  "params": [
    {"group": "form", "name": "input_kind", "kind": "str", "default": "signed", "options": ["signed", "density"], "doc": "Select density when HarmonicChladni outputs nodal density. Its density directly controls organic deposits."},
    {"group": "form", "name": "depth", "kind": "float", "default": 0.22, "min": 0.0, "max": 0.45, "doc": "Height of the harmonic relief. Zero removes harmonic height; texture_depth remains independent."},
    {"group": "form", "name": "seam", "kind": "float", "default": 0.022, "min": 0.008, "max": 0.15, "doc": "Width of the metal seam around zero displacement, relative to range."},
    {"group": "form", "name": "range", "kind": "float", "default": 1.0, "min": 0.01, "max": 20.0, "doc": "Fixed input field range. Sets the relief contrast without normalizing each frame."},
    {"group": "form", "name": "contours", "kind": "float", "default": 14.0, "min": 2.0, "max": 32.0, "doc": "Fine engraved levels across the signed field."},
    {"group": "form", "name": "engraving", "kind": "float", "default": 0.55, "min": 0.0, "max": 1.0, "doc": "Depth and contrast of the contour etching."},
    {"group": "camera", "name": "tilt", "kind": "float", "default": 0.25, "min": 0.0, "max": 0.85, "doc": "View tilt in radians. Higher values reveal the relief."},
    {"group": "camera", "name": "turn", "kind": "float", "default": -0.22, "min": -3.14159, "max": 3.14159, "doc": "Turn the surface in its plane, in radians."},
    {"group": "camera", "name": "zoom", "kind": "float", "default": 1.0, "min": 0.5, "max": 1.6, "doc": "Composition scale. Mirrored field extension keeps every edge filled."},
    {"group": "material", "name": "texture_a", "kind": "str", "default": "sand", "options": ["jade", "brushed metal", "woven silk", "porous stone", "sand", "dunes", "lichen", "coral", "cells", "spores", "pollen", "plankton"], "doc": "Texture at mix zero."},
    {"group": "material", "name": "texture_b", "kind": "str", "default": "lichen", "options": ["jade", "brushed metal", "woven silk", "porous stone", "sand", "dunes", "lichen", "coral", "cells", "spores", "pollen", "plankton"], "doc": "Texture at mix one. Choose any pair of finishes."},
    {"group": "material", "name": "texture_mix", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0, "doc": "Smoothly blend texture height, color, and reflectance. Independent of harmonic structure."},
    {"group": "material", "name": "texture_scale", "kind": "float", "default": 24.0, "min": 4.0, "max": 64.0, "doc": "Texture repeats across the source field. Detail is limited by the height-map resolution."},
    {"group": "material", "name": "texture_depth", "kind": "float", "default": 0.75, "min": 0.0, "max": 1.0, "doc": "Strength of small grains, membranes, grooves, and pores. Does not change the harmonic field."},
    {"group": "material", "name": "density", "kind": "float", "default": 0.65, "min": 0.0, "max": 1.0, "doc": "Coverage of grains and biological features. Increase for dense colonies; decrease for scattered particles."},
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

struct Surface {
    color: vec3f,
    reflectance: vec3f,
    emission: vec3f,
    roughness: f32,
    detail: f32,
    relief: f32,
    gilding: f32,
    sheen: f32,
}

fn cell_hash(q: vec2f) -> vec2f {
    let cell = vec2u(vec2i(q) + 65536);
    var h = cell.x * 1597334673u ^ cell.y * 3812015801u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    return vec2f(f32(h & 65535u), f32(h >> 16u))/65536.0;
}

// Distance to a cell center, distance between its two nearest centers, and cell identity.
fn cells(at: vec2f) -> vec4f {
    var first = 4.0; var second = 4.0; var identity = vec2f(0.0);
    let cell = floor(at);
    for (var y = -1; y <= 1; y = y+1) {
        for (var x = -1; x <= 1; x = x+1) {
            let neighbor = cell + vec2f(f32(x), f32(y));
            let seed = cell_hash(neighbor);
            let distance = length(at-neighbor-0.2-0.6*seed);
            if distance < first {
                second = first; first = distance; identity = seed;
            } else { second = min(second, distance); }
        }
    }
    return vec4f(first, second-first, identity);
}

fn surface(kind: u32, q: vec2f, v: f32) -> Surface {
    let scale = clamp(p.texture_scale, 1.0, max(1.0, min(resolution.x, resolution.y)/8.0));
    let at = (q*0.5+0.5)*scale + select(vec2f(0.0), vec2f(v*1.2, v*0.6), kind >= 4u);
    let tau = 6.28318530718;
    var s = Surface(vec3f(0.0), vec3f(0.045), vec3f(0.0), 0.0, 0.0, 1.0, 1.0, 1.0);
    switch kind {
        case 0u: {
            s.detail = 0.15*sin(at.x*0.73 + sin(at.y*0.51))*sin(at.y*0.61);
            s.color = mix(vec3f(0.042, 0.017, 0.009), vec3f(0.004, 0.031, 0.023), p.patina)*(1.0+s.detail);
        }
        case 1u: {
            s.detail = sin(tau*(at.y + 0.045*sin(at.x*0.8)));
            s.color = vec3f(0.16, 0.095, 0.045)*(0.92+0.08*s.detail);
            s.reflectance = vec3f(0.42, 0.27, 0.13); s.roughness = 0.1;
        }
        case 2u: {
            let warp = cos(tau*at.x); let weft = cos(tau*at.y);
            s.detail = 0.45*(warp+weft) + 0.1*warp*weft;
            s.color = vec3f(0.26, 0.22, 0.15)*(0.86+0.14*s.detail); s.roughness = 0.22;
        }
        case 3u: {
            let cell = cells(at);
            s.detail = 1.0 - 2.0*exp(-cell.x*cell.x*22.0);
            s.color = vec3f(0.065, 0.077, 0.064)*(0.8+0.2*s.detail); s.roughness = 0.38;
        }
        default: {
            // Organic finishes keep field-scale height low and suppress the metal lighting.
            s.relief = 0.13; s.gilding = 0.0; s.sheen = 0.12;
            s.roughness = 0.44; s.reflectance = vec3f(0.012);
            let density = clamp(p.density, 0.0, 1.0);
            let band = max(abs(p.seam)*3.0, 0.003);
            let nodal = exp(-v*v/(2.0*band*band));
            let fine = cells(at*2.0);
            let coarse = cells(at*0.65);
            let grain = 1.0-smoothstep(0.05, 0.21+0.13*density, fine.x);
            let deposit = smoothstep(fine.z-0.1, fine.z+0.1, density*(0.035+1.3*nodal));
            let particles = grain*deposit;
            let dust = cell_hash(floor(at*8.0)).x;
            switch kind {
                case 4u: { // Sand: separate angular grains collect along the nodal field.
                    let tint = mix(vec3f(0.17, 0.09, 0.034), vec3f(0.57, 0.40, 0.19), fine.w);
                    s.color = mix(vec3f(0.017, 0.013, 0.009)*(0.7+0.6*dust), tint, particles);
                    s.detail = 1.7*particles + 0.08*dust; s.relief = 0.07;
                }
                case 5u: { // Dunes: narrow wind ridges with fine dry grains.
                    let ripple = pow(0.5+0.5*sin(tau*(at.y*0.7+0.22*sin(at.x*0.4)+v*0.8)), 5.0);
                    s.color = mix(vec3f(0.11, 0.045, 0.012), vec3f(0.42, 0.25, 0.10), 0.35+0.65*ripple)*(0.78+0.44*dust);
                    s.detail = ripple*1.5+grain*0.12; s.relief = 0.3;
                }
                case 6u: { // Lichen: uneven colonies over a dark, rough substrate.
                    let colony = smoothstep(0.35, 0.75, density*0.4+0.8*nodal+0.15*sin(at.x*0.31+sin(at.y*0.43)));
                    let lobes = 1.0-smoothstep(0.24, 0.55, coarse.x+0.06*sin(at.x*8.0)*sin(at.y*7.0));
                    let growth = colony*lobes;
                    let green = mix(vec3f(0.04, 0.095, 0.018), vec3f(0.34, 0.39, 0.06), fine.z);
                    s.color = mix(vec3f(0.018, 0.023, 0.017), green, growth)*(0.6+0.8*grain);
                    s.detail = 1.3*growth+0.35*grain; s.roughness = 0.5;
                }
                case 7u: { // Coral: irregular porous walls and warm inner chambers.
                    let wall = 1.0-smoothstep(0.035, 0.17+0.12*density, coarse.y);
                    let pores = 1.0-smoothstep(0.02, 0.18, fine.x);
                    s.color = mix(vec3f(0.052, 0.012, 0.009), vec3f(0.43, 0.16, 0.08), wall)*(0.85-0.55*pores)*(0.4+0.6*nodal);
                    s.detail = 1.6*wall-0.45*pores; s.relief = 0.22;
                }
                case 8u: { // Cells: thin membranes and small nuclei, with dark interiors.
                    let membrane = 1.0-smoothstep(0.012, 0.075, coarse.y);
                    let nucleus = exp(-coarse.x*coarse.x*150.0);
                    s.color = mix(vec3f(0.012, 0.033, 0.043), vec3f(0.09, 0.22, 0.19), membrane)*(0.7+0.5*coarse.z);
                    s.color += vec3f(0.15, 0.07, 0.035)*nucleus;
                    s.emission = vec3f(0.015, 0.065, 0.06)*membrane*density*(0.2+nodal);
                    s.detail = membrane*0.6+nucleus*0.25; s.relief = 0.09;
                }
                case 9u: { // Spores: scattered grains with a fine outer rim.
                    let rim = exp(-pow((fine.x-0.19)/0.05, 2.0))*deposit;
                    s.color = vec3f(0.009, 0.014, 0.015)+particles*mix(vec3f(0.09, 0.13, 0.06), vec3f(0.33, 0.29, 0.16), fine.w);
                    s.color += rim*vec3f(0.09, 0.12, 0.07);
                    s.detail = particles*1.2+rim*0.35; s.relief = 0.04;
                }
                case 10u: { // Pollen: warm clustered grains with a stippled coat.
                    let cluster = smoothstep(0.12, 0.7, density*0.3+nodal-0.25*coarse.z);
                    let pollen = grain*cluster;
                    s.color = mix(vec3f(0.025, 0.013, 0.012), vec3f(0.48, 0.21, 0.025)*(0.65+0.7*dust), pollen);
                    s.detail = pollen*(1.2+0.4*dust); s.relief = 0.08;
                }
                default: { // Plankton: sparse cool light points in a dark field.
                    let point = exp(-fine.x*fine.x*95.0)*deposit;
                    let halo = exp(-fine.x*fine.x*14.0)*deposit;
                    s.color = vec3f(0.003, 0.011, 0.018)+point*vec3f(0.015, 0.11, 0.12);
                    s.emission = mix(vec3f(0.025, 0.48, 0.38), vec3f(0.08, 0.22, 0.7), fine.w)*(point*1.3+halo*0.1);
                    s.detail = point*0.3; s.relief = 0.03; s.sheen = 0.0;
                }
            }
        }
    }
    return s;
}

fn blended_surface(q: vec2f, v: f32) -> Surface {
    let a = surface(p.texture_a, q, v);
    if p.texture_a == p.texture_b || p.texture_mix <= 0.0 { return a; }
    let b = surface(p.texture_b, q, v);
    if p.texture_mix >= 1.0 { return b; }
    let t = smoothstep(0.0, 1.0, p.texture_mix);
    return Surface(mix(a.color, b.color, t), mix(a.reflectance, b.reflectance, t), mix(a.emission, b.emission, t),
        mix(a.roughness, b.roughness, t), mix(a.detail, b.detail, t), mix(a.relief, b.relief, t),
        mix(a.gilding, b.gilding, t), mix(a.sheen, b.sheen, t));
}

fn field_at(q: vec2f) -> vec2f {
    var value = textureSampleLevel(input, samp, mirrored_uv(q), 0.0);
    if goofi_geo_valid(geometry) { value = goofi_geo_field(geometry, mirrored_uv(q)); }
    // Saturate extreme external fields before any square or contour calculation.
    var v = clamp(value.r / max(abs(p.range), 0.0001), -8.0, 8.0);
    if p.input_kind == 1u {
        // Invert the deposit Gaussian so density is not transformed twice.
        let band = max(abs(p.seam)*3.0, 0.003);
        v = band*sqrt(-2.0*log(clamp(value.r, 0.00000001, 1.0)));
    }
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
    let finish = blended_surface(q, v);
    let height = 0.035 + clamp(p.depth, 0.0, 0.45)*finish.relief*(0.16 + 0.35/(1.0+3.0*v*v) + 0.32*shoulder + 0.13*crest
        - 0.022*p.engraving*etched*(1.0-crest));
    return mix(-0.035, height + 0.008*clamp(p.texture_depth, 0.0, 1.0)*finish.detail, sample.y);
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
        let finish = blended_surface(mirrored_uv(q)*2.0-1.0, v);
        let gilding = clamp(p.metal, 0.0, 1.0)*seam*finish.gilding;
        let basin = finish.color * mix(0.5, 1.4, smoothstep(-0.6, 0.7, v));
        let gold = vec3f(0.48, 0.31, 0.12);
        let albedo = mix(basin, gold, gilding);
        let etched = pow(0.5+0.5*cos(v*p.contours*6.28318530718), 16.0)*(1.0-seam);
        let occlusion = 0.6 + 0.4/(1.0+length(slope)*0.7);
        let diffuse = max(dot(normal, light), 0.0);
        let rough = pow(clamp(p.roughness + finish.roughness*(1.0-gilding), 0.12, 0.9), 2.0);
        let nh = max(dot(normal, halfway), 0.0);
        let distribution = rough*rough / (3.14159265359*pow(nh*nh*(rough*rough-1.0)+1.0, 2.0));
        let reflectance = mix(finish.reflectance, gold, gilding);
        let fresnel = reflectance + (1.0-reflectance)*pow(1.0-max(dot(halfway, view), 0.0), 5.0);
        let specular = fresnel*distribution*diffuse/(0.35+4.0*max(dot(normal, view), 0.0));
        let sky = pow(max(dot(normal, normalize(vec3f(-0.4, 0.8, 0.65))), 0.0), 3.0);
        let rim = pow(1.0-max(dot(normal, view), 0.0), 3.0);
        color = albedo*(0.45*occlusion + 2.7*diffuse*mix(0.25, 1.0, shadow));
        color += (specular*shadow*2.0 + vec3f(0.025, 0.075, 0.070)*sky*(0.3+0.7*gilding))*finish.sheen;
        color += vec3f(0.16, 0.22, 0.19)*rim*occlusion*finish.sheen;
        color *= 1.0 - p.engraving*etched*0.42*finish.gilding;
        color *= 0.97+0.06*grain(q);
        color += finish.emission;
        color = mix(backdrop, color, coverage);
    }
    color *= 1.0 - 0.14*smoothstep(0.3, 2.0, dot(uv-0.5, uv-0.5)*4.0);
    let mapped = 1.0-exp(-max(color, vec3f(0.0))*max(p.exposure, 0.0));
    return vec4f(pow(mapped, vec3f(1.0/2.2)), 1.0);
}
