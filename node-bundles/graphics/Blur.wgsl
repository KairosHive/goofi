/* goofi
{ "doc": "soften an image or spread it along a path\nGaussian weights the center; box averages a square; disk averages a circle. Directional follows angle in degrees (0 is horizontal). Radial rotates about center x/y; zoom moves toward and away from that center. Radius is the maximum offset as a fraction of image height, or a fraction of a turn for radial, or a scale change for zoom. Gaussian uses radius as three standard deviations. Quality sets 5, 9, or 17 samples per axis or path; higher quality reduces gaps at large radii but costs more GPU time. Frame edges are extended. Colors are averaged with alpha weights; radius zero passes the input through.",
  "tags": ["image", "transform"],
  "inputs": [{"name": "input", "kind": "TEXTURE"}],
  "params": [
    {"group": "blur", "name": "mode", "kind": "str", "default": "gaussian", "options": ["gaussian", "box", "disk", "directional", "radial", "zoom"]},
    {"group": "blur", "name": "radius", "kind": "float", "default": 0.005, "min": 0.0, "max": 0.25},
    {"group": "blur", "name": "quality", "kind": "str", "default": "medium", "options": ["low", "medium", "high"]},
    {"group": "blur", "name": "angle", "kind": "float", "default": 0.0, "min": -180.0, "max": 180.0},
    {"group": "blur", "name": "center_x", "kind": "float", "default": 0.5, "min": 0.0, "max": 1.0},
    {"group": "blur", "name": "center_y", "kind": "float", "default": 0.5, "min": 0.0, "max": 1.0} ] }
*/
fn blur_texel(at: vec2i, size: vec2i) -> vec4f {
    let color = textureLoad(input, clamp(at, vec2i(0), size - vec2i(1)), 0);
    let alpha = clamp(color.a, 0.0, 1.0);
    return vec4f(color.rgb * alpha, alpha);
}

fn blur_sample(uv: vec2f) -> vec4f {
    // Convert to premultiplied color before interpolation to exclude hidden RGB.
    let size = vec2i(textureDimensions(input));
    let position = clamp(uv * vec2f(size) - 0.5, vec2f(0.0), vec2f(size - vec2i(1)));
    let at = vec2i(floor(position));
    let fraction = fract(position);
    return mix(mix(blur_texel(at, size), blur_texel(at + vec2i(1, 0), size), fraction.x),
               mix(blur_texel(at + vec2i(0, 1), size), blur_texel(at + vec2i(1, 1), size), fraction.x), fraction.y);
}

fn shade(uv: vec2f) -> vec4f {
    if p.radius == 0.0 { return textureSampleLevel(input, samp, uv, 0.0); }
    let half_count = 2i << p.quality;
    let size = vec2f(textureDimensions(input));
    let aspect = vec2f(size.y / size.x, 1.0);
    var sum = vec4f(0.0);
    var weight_sum = 0.0;
    if p.mode <= 2u {
        for (var y = -half_count; y <= half_count; y++) {
            for (var x = -half_count; x <= half_count; x++) {
                let offset = vec2f(f32(x), f32(y)) / f32(half_count);
                let distance_squared = dot(offset, offset);
                if p.mode == 2u && distance_squared > 1.0 { continue; }
                var weight = 1.0;
                if p.mode == 0u { weight = exp(-4.5 * distance_squared); }
                sum += blur_sample(uv + offset * p.radius * aspect) * weight;
                weight_sum += weight;
            }
        }
    } else {
        let center = vec2f(p.center_x, p.center_y);
        let relative = (uv - center) / aspect;
        let direction = vec2f(cos(radians(p.angle)), sin(radians(p.angle)));
        for (var i = -half_count; i <= half_count; i++) {
            let offset = f32(i) / f32(half_count) * p.radius;
            var q = uv + direction * offset * aspect;
            if p.mode == 4u {
                let angle = offset * 6.28318530718;
                let turned = vec2f(relative.x * cos(angle) - relative.y * sin(angle),
                                   relative.x * sin(angle) + relative.y * cos(angle));
                q = center + turned * aspect;
            } else if p.mode == 5u {
                q = center + (uv - center) * (1.0 + offset);
            }
            sum += blur_sample(q);
            weight_sum += 1.0;
        }
    }
    if sum.a <= 0.0 { return vec4f(0.0); }
    return vec4f(sum.rgb / sum.a, sum.a / weight_sum);
}
