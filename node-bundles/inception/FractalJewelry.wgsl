/* goofi
{"doc":"Recursive jeweled filigree, from icy spiral lace to enameled paisley.\nEach recursion level deposits its own ribbon, beads and lace on top of the coarser ones, so the frame fills with nested ornament. topology x fold x symmetry x palette are a combinatorial space of forms; style blends the cold filament look (0) into gold-and-teal enamel (1). Set wander to 0 for a still image.","tags":["image","generator","simulation"],"params":[
{"group":"form","name":"topology","kind":"str","default":"vortex","options":["vortex","rosette","paisley","labyrinth","lattice","nautilus"]},
{"group":"form","name":"fold","kind":"str","default":"mirror","options":["none","abs","mirror","sphere","tile"]},
{"group":"form","name":"symmetry","kind":"int","default":5,"min":1,"max":12},
{"group":"form","name":"curl","kind":"float","default":1.2,"min":-3,"max":3},
{"group":"form","name":"recursion","kind":"int","default":9,"min":2,"max":16},
{"group":"form","name":"contraction","kind":"float","default":1.35,"min":1.02,"max":2.2},
{"group":"form","name":"trap","kind":"str","default":"circle","options":["circle","polygon","cross","petal","box","lattice","cell"]},
{"group":"form","name":"inversion","kind":"float","default":0,"min":0,"max":1},
{"group":"bend","name":"mobius","kind":"float","default":0,"min":0,"max":1},
{"group":"bend","name":"swirl","kind":"float","default":0,"min":-2,"max":2},
{"group":"bend","name":"lens","kind":"float","default":0,"min":-1,"max":1},
{"group":"bend","name":"unfurl","kind":"float","default":0,"min":0,"max":1},
{"group":"bend","name":"depth","kind":"float","default":0,"min":0,"max":1},
{"group":"life","name":"veins","kind":"float","default":0,"min":0,"max":1},
{"group":"life","name":"pulse","kind":"float","default":0,"min":0,"max":1},
{"group":"life","name":"undulate","kind":"float","default":0,"min":0,"max":1},
{"group":"life","name":"grow","kind":"float","default":0,"min":0,"max":1},
{"group":"form","name":"weave","kind":"str","default":"rings","options":["ring","rings","offsets","spiral"]},
{"group":"morph","name":"topology_b","kind":"str","default":"paisley","options":["vortex","rosette","paisley","labyrinth","lattice","nautilus"]},
{"group":"morph","name":"blend","kind":"float","default":0,"min":0,"max":1},
{"group":"morph","name":"warp","kind":"float","default":0,"min":0,"max":1},
{"group":"morph","name":"twist","kind":"float","default":0,"min":-2,"max":2},
{"group":"morph","name":"pinch","kind":"float","default":1,"min":0.4,"max":2.2},
{"group":"morph","name":"asym","kind":"float","default":0,"min":0,"max":1},
{"group":"form","name":"flow","kind":"float","default":0.55,"min":0,"max":1},
{"group":"form","name":"seed","kind":"float","default":2.3,"min":0,"max":100},
{"group":"form","name":"zoom","kind":"float","default":1.0,"min":0.2,"max":4},
{"group":"material","name":"palette","kind":"str","default":"frost","options":["frost","enamel","peacock","ember","opal","bone"]},
{"group":"material","name":"finish","kind":"str","default":"metal","options":["metal","enamel","glass","iridescent","stone"]},
{"group":"material","name":"style","kind":"float","default":0.6,"min":0,"max":1},
{"group":"material","name":"hue","kind":"float","default":0.0,"min":0,"max":1},
{"group":"material","name":"spread","kind":"float","default":0.5,"min":0,"max":1},
{"group":"material","name":"relief","kind":"float","default":0.85,"min":0,"max":1},
{"group":"material","name":"glow","kind":"float","default":0.5,"min":0,"max":2},
{"group":"ornament","name":"motif","kind":"str","default":"beads","options":["beads","scales","spikes","eyes","chain","plain"]},
{"group":"ornament","name":"ground","kind":"str","default":"mosaic","options":["mosaic","nebula","silk","void"]},
{"group":"ornament","name":"jewels","kind":"float","default":0.8,"min":0,"max":1},
{"group":"ornament","name":"lace","kind":"float","default":0.7,"min":0,"max":1},
{"group":"ornament","name":"mosaic","kind":"float","default":0.5,"min":0,"max":1},
{"group":"ornament","name":"density","kind":"float","default":1.0,"min":0.3,"max":3},
{"group":"ornament","name":"width","kind":"float","default":1.0,"min":0.2,"max":3},
{"group":"motion","name":"wander","kind":"float","default":0.25,"min":0,"max":1},
{"group":"motion","name":"rate","kind":"float","default":0.06,"min":0,"max":0.5},
{"group":"motion","name":"drift","kind":"float","default":0,"min":-1,"max":1},
{"group":"motion","name":"breathe","kind":"float","default":0,"min":0,"max":1},
{"group":"motion","name":"stream","kind":"float","default":0,"min":-2,"max":2},
{"group":"motion","name":"churn","kind":"float","default":0,"min":0,"max":1}
]}
*/
const TAU: f32 = 6.2831853;

fn rot(v: vec2f, a: f32) -> vec2f {
    let c = cos(a); let s = sin(a);
    return vec2f(c * v.x - s * v.y, s * v.x + c * v.y);
}

fn hash1(x: f32) -> f32 {
    return fract(sin(x * 127.1 + 74.7) * 43758.5453);
}

fn hash2(v: vec2f) -> f32 {
    return fract(sin(dot(v, vec2f(127.1, 311.7))) * 43758.5453);
}

// Six cosine palettes; the same phase set is reused so `hue` slides through any of them.
fn palette(x: f32) -> vec3f {
    var a = vec3f(0.5); var b = vec3f(0.5); var c = vec3f(1.0); var d = vec3f(0.0, 0.33, 0.67);
    switch p.palette {
        case 1u: { a = vec3f(0.55, 0.42, 0.32); b = vec3f(0.45, 0.42, 0.35); d = vec3f(0.02, 0.35, 0.58); }
        case 2u: { a = vec3f(0.35, 0.5, 0.5); b = vec3f(0.35, 0.45, 0.45); d = vec3f(0.55, 0.42, 0.15); }
        case 3u: { a = vec3f(0.6, 0.35, 0.22); b = vec3f(0.4, 0.32, 0.22); d = vec3f(0.02, 0.15, 0.32); }
        case 4u: { a = vec3f(0.5, 0.5, 0.55); b = vec3f(0.45, 0.4, 0.45); c = vec3f(1.0, 0.9, 1.1); d = vec3f(0.15, 0.45, 0.78); }
        case 5u: { a = vec3f(0.62, 0.58, 0.52); b = vec3f(0.22, 0.22, 0.25); d = vec3f(0.05, 0.12, 0.2); }
        default: { a = vec3f(0.42, 0.48, 0.58); b = vec3f(0.35, 0.32, 0.32); d = vec3f(0.62, 0.55, 0.42); }
    }
    return clamp(a + b * cos(TAU * (c * x + d)), vec3f(0.0), vec3f(1.4));
}

// One step of the recursive map: a fold, then a topology-specific twist and contraction.
fn step_map(q: vec2f, k: f32, seed: f32, conformal: bool, topology: u32) -> vec2f {
    var v = q;
    let sym = f32(max(p.symmetry, 1));

    if conformal {
        // Angle-preserving step: inversion, spiral twist and a shift. It never creases
        // the plane, so the coarse levels come out as long unbroken sweeping ribbons.
        let d2 = dot(v, v);
        if d2 > 1.0 { v = v / d2; }          // circle inversion: everything falls inward
        let r = max(length(v), 1e-4);
        v = rot(v, p.curl * log(r + 0.28) + TAU * k / sym + seed * 0.2);
        v = v * p.contraction - vec2f(0.42, 0.13);
        return v;
    }

    // Box + ball fold (Mandelbox): draws the whole plane into the attractor, so the
    // frame fills with structure instead of leaving a bare surround.
    v = clamp(v, vec2f(-1.0), vec2f(1.0)) * 2.0 - v;
    let r2 = dot(v, v);
    if r2 < 0.28 { v = v * 3.6; } else if r2 < 1.0 { v = v / r2; }

    switch p.fold {
        case 1u: { v = abs(v); }
        case 2u: {
            let a = atan2(v.y, v.x);
            let r = length(v);
            let wedge = (abs(fract(a / TAU * sym + 0.5) - 0.5)) * TAU / sym;
            v = vec2f(cos(wedge), sin(wedge)) * r;
        }
        case 3u: {
            let d2 = max(dot(v, v), 0.06);
            v = v * clamp(1.0 / d2, 0.4, 2.4);
        }
        case 4u: { v = fract(v * 0.5 + 0.5) * 2.0 - 1.0; }
        default: {}
    }

    var r = max(length(v), 1e-4);
    // Radial pinch and an angle-driven twist: two smooth ways to bend a form without
    // changing which form it is.
    if p.pinch != 1.0 { v = v * (pow(r, p.pinch) / r); r = max(length(v), 1e-4); }
    if p.twist != 0.0 { v = rot(v, p.twist * r); }
    if p.asym != 0.0 { v = v + vec2f(p.asym * 0.22 * sin(k * 1.7), p.asym * 0.17 * cos(k * 2.3)); }
    switch topology {
        case 1u: { v = rot(v, TAU / sym + p.curl * 0.5) - vec2f(0.42, 0.0); }
        case 2u: { v = rot(v, p.curl * 1.6 / (0.35 + r)); v.x += 0.3 * sin(v.y * 2.2 + seed); }
        case 3u: { v = rot(abs(v) - vec2f(0.5, 0.34), p.curl * 0.7 + seed * 0.05); }
        case 4u: { v = rot(v, p.curl * 0.4 * k) - vec2f(0.36, 0.28); v = abs(v) - vec2f(0.18, 0.0); }
        case 5u: { v = rot(v, p.curl * log(r + 0.25) + k * 0.19); v -= vec2f(0.5, 0.0); }
        default: { v = rot(v, p.curl * log(r + 0.2) + seed * 0.25) - vec2f(0.55, 0.12); }
    }
    return v * p.contraction;
}

// Four continuous bends of the plane. Each is the identity at 0 and deforms smoothly
// from there, so any of them can be swept or driven without a seam appearing.
fn bend(v: vec2f, amt: f32, seed: f32) -> vec2f {
    var z = v;
    if p.mobius * amt > 0.001 {
        // Mobius map (z + b) / (c z + 1) in the complex plane: it bends straight
        // ribbons into loxodromic spirals without ever tearing them.
        let m = p.mobius * amt;
        let b = 0.45 * m * vec2f(cos(seed * 0.7), sin(seed * 0.7));
        let c = 0.55 * m * vec2f(sin(seed * 0.5), cos(seed * 0.5));
        let num = z + b;
        let den = vec2f(c.x * z.x - c.y * z.y + 1.0, c.x * z.y + c.y * z.x);
        let dd = max(dot(den, den), 1e-4);
        z = vec2f(num.x * den.x + num.y * den.y, num.y * den.x - num.x * den.y) / dd;
    }
    if p.swirl * amt != 0.0 {
        z = rot(z, p.swirl * amt * length(z));          // rotation that grows with radius
    }
    if p.lens * amt != 0.0 {
        let r = max(length(z), 1e-4);
        z = z * (1.0 + p.lens * amt * 0.35 * sin(r * 3.4));   // concentric lensing
    }
    if p.unfurl * amt > 0.001 {
        // Toward log-polar: rings unroll into parallel bands as this rises.
        let r = max(length(z), 1e-4);
        z = mix(z, vec2f(log(r) * 0.8, atan2(z.y, z.x) * 0.55), p.unfurl * amt);
    }
    return z;
}

// Venation: ridged filaments, each octave warped by the one before it, which is how a
// leaf vein branches off its own parent.
fn veins(v: vec2f, seed: f32) -> f32 {
    var z = v;
    var acc = 0.0;
    var amp = 1.0;
    for (var j = 0; j < 3; j++) {
        let ridge = 1.0 - abs(sin(z.x * 2.3 + seed) * sin(z.y * 2.7 - seed));
        acc += amp * pow(ridge, 6.0);
        z = rot(z * 1.9, 1.1) + vec2f(ridge * 0.6, -ridge * 0.4);
        amp *= 0.55;
    }
    return clamp(acc, 0.0, 1.0);
}

// The trap the ribbon is drawn around. Every shape returns a signed distance in the
// iterate's own coordinates; the ribbon is the set where that distance is near zero,
// so swapping the shape changes what the whole nest is made of.
fn trap_dist(v: vec2f, k: f32, seed: f32, minc: f32) -> f32 {
    let sym = f32(max(p.symmetry, 1));
    let rr = max(length(v), 1e-4);
    let ang = atan2(v.y, v.x);
    let lobe = 0.09 * sin(ang * sym + seed + k * 0.4);
    // Growth: tips reach out and draw back, so the form buds rather than spins.
    let radius = 0.62 + lobe + p.grow * 0.22 * sin(ang * sym * 0.5 - time * p.rate * TAU * 0.5 + k * 0.7);
    switch p.trap {
        case 1u: {                                   // regular polygon
            let wedge = TAU / sym;
            let a = abs(fract(ang / wedge + 0.5) - 0.5) * wedge;
            return rr * cos(a) / cos(wedge * 0.5) - radius;
        }
        case 2u: {                                   // crossed bars
            return min(abs(v.x), abs(v.y)) - radius * 0.28;
        }
        case 3u: {                                   // petals
            return rr - radius * (0.55 + 0.5 * abs(sin(ang * sym * 0.5)));
        }
        case 4u: {                                   // square
            let e = abs(v) - vec2f(radius);
            return length(max(e, vec2f(0.0))) + min(max(e.x, e.y), 0.0);
        }
        case 5u: {                                   // lattice of cells
            let c = abs(fract(v * (1.0 + sym * 0.25)) - 0.5) / (1.0 + sym * 0.25);
            return min(c.x, c.y) - radius * 0.12;
        }
        case 6u: {                                   // soft tissue cells
            let g = veins(v * min(0.85, 0.35 / max(minc, 0.004)), seed);
            return (0.5 - g) * radius * 1.6;
        }
        default: { return rr - radius; }             // circle
    }
}

fn shade(uv: vec2f) -> vec4f {
    let t = time * p.rate;
    let seed = p.seed + p.wander * 0.6 * sin(t * 0.61);
    let aspect = resolution.x / max(resolution.y, 1.0);
    // Every time term is a continuous function of t, so the motion never steps.
    let breath = 1.0 + 0.28 * p.breathe * sin(t * TAU * 0.31);
    var q = (uv - 0.5) * vec2f(aspect, 1.0) * 2.6 / max(p.zoom * breath, 0.1);
    q = rot(q, 0.25 * p.wander * sin(t * 0.37) + p.drift * t * TAU * 0.25);
    if p.inversion > 0.001 {
        // Circle inversion of the view: the far field folds inward and the whole
        // structure turns itself inside out as this rises.
        let d2 = max(dot(q, q), 0.02);
        q = mix(q, q / d2 * 0.9, p.inversion);
    }
    // The bend applies to the view; `depth` re-applies it at every recursion level,
    // which makes the deformation itself fractal instead of one lens over the top.
    q = bend(q, 1.0, seed);
    if p.warp != 0.0 {
        // Domain warp: a slow swell that bends the whole field as it moves.
        q += p.warp * 0.45 * vec2f(sin(q.y * 1.3 + t * 0.9), cos(q.x * 1.5 - t * 0.7));
    }

    let light = normalize(vec3f(-0.45, -0.62, 0.65));
    let half_vec = normalize(light + vec3f(0.0, 0.0, 1.0));
    let px = 2.6 / max(resolution.y, 1.0) / max(p.zoom, 0.1);

    // Ground: the field the ornament sits on. Each option fills the frame in its own
    // way, so nothing is ever bare behind the ribbons.
    let warp2 = q + vec2f(sin(q.y * 1.7 + seed), cos(q.x * 1.9 - seed)) * 0.55
                  + vec2f(sin(q.y * 4.3), cos(q.x * 3.7)) * 0.18;
    var col = vec3f(0.0);
    switch p.ground {
        case 1u: {   // nebula: soft clouded depth
            let f1 = 0.5 + 0.5 * sin(warp2.x * 2.1 + warp2.y * 1.3 + seed);
            let f2 = 0.5 + 0.5 * sin(warp2.y * 3.7 - warp2.x * 2.3 - seed * 0.5);
            col = palette(p.hue + 0.5 + 0.25 * p.spread * f1) * (0.03 + 0.11 * f1 * f2);
        }
        case 2u: {   // silk: fine drawn threads catching the light
            let sheen = 0.5 + 0.5 * sin(warp2.x * 26.0 * p.density + warp2.y * 5.0);
            col = palette(p.hue + 0.55) * (0.035 + 0.06 * pow(sheen, 3.0));
        }
        case 3u: { col = palette(p.hue + 0.5) * 0.012; }   // void: near black
        default: {   // mosaic: cracked inlay cells
            let mg = (2.2 + 4.5 * p.mosaic) * p.density;
            let cell = floor(warp2 * mg);
            let crack = fract(warp2 * mg) - 0.5;
            let seam = 1.0 - smoothstep(0.28, 0.5, max(abs(crack.x), abs(crack.y)));
            col = palette(p.hue + hash2(cell) * 0.3 * p.spread + 0.5) * (0.06 + 0.08 * hash2(cell + 3.1));
            col = mix(col * 0.45, col, mix(1.0, seam, p.mosaic));
        }
    }
    col += palette(p.hue + 0.1) * 0.05 * exp(-dot(q, q) * 0.3);

    var scale = 1.0;   // accumulated contraction, so world width shrinks with depth
    var haze = 0.0;

    for (var i = 0; i < 16; i++) {
        if i >= p.recursion { break; }
        let k = f32(i);
        let conformal = k < round(p.flow * f32(p.recursion));
        let churn = p.churn * 0.5 * sin(t * TAU * 0.23 + k * 1.1);   // levels drift apart
        var qn = step_map(rot(q, churn), k, seed, conformal, p.topology);
        if p.blend > 0.001 {
            // Crossfade between two topologies: the form morphs continuously rather
            // than snapping from one to the other.
            qn = mix(qn, step_map(rot(q, churn), k, seed, conformal, p.topology_b), p.blend);
        }
        q = qn;
        if p.depth > 0.001 { q = bend(q, p.depth * 0.6, seed + k); }
        scale *= select(p.contraction, mix(1.0, p.contraction, 0.45), conformal);

        // Trap distance in iterate coordinates, then the weave decides whether that
        // is one ribbon or a whole family of offsets of it.
        let ang = atan2(q.y, q.x);
        let rr = max(length(q), 1e-4);
        let n = 0.6 + 1.2 * p.density;
        let minc = px * scale;
        let b = trap_dist(q, k, seed, minc);
        var dq = b;                                   // signed distance, iterate units
        var pitch_q = 0.62;                           // gap to the next ribbon
        if p.weave == 1u {
            // Self-similar rings: spacing grows with radius, so the nest is scale-free.
            let lr = log(rr) * n + b * 3.0 + k * 0.3 + p.stream * t;
            dq = (fract(lr) - 0.5) * rr / n;
            pitch_q = rr / n;
        } else if p.weave == 2u {
            // Even offsets of the trap shape: parallel contours at a fixed spacing,
            // which is what turns a polygon or a cross into a woven field.
            let gap = 0.55 / n;
            dq = (fract(b / gap + p.stream * t) - 0.5) * gap;
            pitch_q = gap;
        } else if p.weave == 3u {
            // The same family sheared by the angle: contours open into a spiral.
            let lr = log(rr) * n + ang * p.curl * 0.5 + b * 3.0 + p.stream * t;
            dq = (fract(lr) - 0.5) * rr / n;
            pitch_q = rr / n;
        }
        let d = dq / scale;
        let pitch = pitch_q / scale;

        // The ribbon's normal follows the trap's own gradient, so a square reads as a
        // square and a cross keeps its corners.
        let eps = 0.004;
        let dir = normalize(vec2f(trap_dist(q + vec2f(eps, 0.0), k, seed, minc) - b,
                                  trap_dist(q + vec2f(0.0, eps), k, seed, minc) - b) + vec2f(1e-6));

        // The ribbon is never wider than the gap to its neighbour, and a level whose
        // ribbons are packed tighter than a pixel fades out instead of aliasing.
        // Peristalsis: a thickness wave travelling along the ribbon, the way a gut
        // moves; undulation slides it sideways like a swimming cilium.
        let along = ang * (3.0 + 6.0 * p.density) - t * TAU;
        let swell = 1.0 + 0.55 * p.pulse * sin(along);
        let w = min(p.width * 0.055 * swell / scale, pitch * 0.42);
        let aa = max(px, 1e-5);
        haze += exp(-abs(d) / max(w * 6.0, aa)) * 0.12;

        let d_bio = d + p.undulate * 0.45 * w * sin(along * 1.7 + rr * 4.0);
        let x = clamp(d_bio / max(w, aa), -1.0, 1.0);
        let cover = 1.0 - smoothstep(w - aa, w + aa, abs(d_bio));
        if cover > 0.002 {
            // Tube profile -> rounded metal, the raised ribbon of both references.
            let h = sqrt(max(1e-4, 1.0 - x * x));
            let n = normalize(vec3f(-dir * x * (p.relief * 1.6) / h, 1.0));
            let dif = max(dot(n, light), 0.0);
            let spec = pow(max(dot(n, half_vec), 0.0), 42.0);

            let tint = palette(p.hue + p.spread * (k * 0.11 + ang * 0.06 + hash1(k + 7.0) * 0.3));
            let icy = mix(vec3f(0.36, 0.52, 0.72), tint, 0.35);
            // Ornament only where a bead or a hatch is still bigger than a pixel;
            // deeper levels keep their plain metal and stay clean instead of fizzing.
            let arc = TAU * rr / (scale * aa);   // ribbon length in pixels, one turn
            let fine = smoothstep(3.0, 9.0, arc / (14.0 + 22.0 * p.density));
            let warm = mix(vec3f(0.85, 0.6, 0.22), tint, 0.55);
            let metal = mix(icy, warm, p.style);

            // Finish decides how the ribbon takes light: the same geometry can read as
            // cast metal, poured enamel, cut glass, oil-film or dry stone.
            var ribbon = metal * (0.12 + 0.95 * dif) + vec3f(1.0, 0.94, 0.82) * spec * (0.35 + 0.65 * p.style);
            switch p.finish {
                case 1u: {   // enamel: flat colour, one tight highlight, dark seam
                    ribbon = metal * (0.45 + 0.5 * dif) * (0.75 + 0.25 * h)
                           + vec3f(1.0) * pow(max(dot(n, half_vec), 0.0), 90.0) * 0.8;
                }
                case 2u: {   // glass: bright rims, thin body, light passing through
                    let edge = pow(1.0 - h, 2.5);
                    ribbon = metal * 0.25 + vec3f(0.8, 0.9, 1.0) * edge * 0.9
                           + vec3f(1.0) * spec * 1.4;
                }
                case 3u: {   // iridescent: hue turns with the viewing angle
                    let film = palette(p.hue + h * 0.5 + dif * 0.35 + k * 0.05);
                    ribbon = film * (0.3 + 0.75 * dif) + vec3f(1.0) * spec * 0.9;
                }
                case 4u: {   // stone: matte, grainy, no specular
                    let grain = 0.85 + 0.3 * hash2(floor(q * 260.0));
                    ribbon = metal * (0.3 + 0.7 * dif) * grain;
                }
                default: {}
            }

            // Lace: fine cross-hatch chasing along the ribbon.
            let s = ang * (18.0 + 26.0 * p.density) + k * 2.0;
            let hatch = pow(0.5 + 0.5 * cos(s), 8.0) * pow(0.5 + 0.5 * cos(s * 1.7 + x * 4.0), 4.0);
            ribbon += p.lace * smoothstep(2.0, 6.0, arc / (18.0 + 26.0 * p.density)) * hatch * mix(vec3f(0.55, 0.8, 1.0), vec3f(0.95, 0.82, 0.5), p.style) * 0.5 * h;

            // Motif: the repeating ornament threaded along the ribbon. Each one uses
            // the same cell coordinates, so they all follow the weave exactly.
            let per_turn = 14.0 + 22.0 * p.density;
            let cn = floor(ang / TAU * per_turn + k * 3.7);
            let cf = fract(ang / TAU * per_turn + k * 3.7) - 0.5;
            let gem = palette(p.hue + 0.35 + p.spread * hash1(cn + k * 13.0));
            var shape = 0.0;
            var nb = vec3f(0.0, 0.0, 1.0);
            switch p.motif {
                case 1u: {   // scales: overlapping fans, each lit from its own centre
                    let sr = length(vec2f(cf * 1.5, (x + 0.55) * 0.8));
                    shape = 1.0 - smoothstep(0.55, 0.72, sr);
                    nb = normalize(vec3f(cf * 1.8, x + 0.55, 0.9));
                }
                case 2u: {   // spikes: frost crystals standing off the ribbon
                    let sp = abs(cf) * 2.4 + max(0.0, x) * 0.7;
                    shape = (1.0 - smoothstep(0.35, 0.95, sp)) * step(0.0, x);
                    nb = normalize(vec3f(cf * 3.0, 0.4, 0.55));
                }
                case 3u: {   // eyes: a ring with a pupil, the paisley motif
                    let er = length(vec2f(cf * 1.9, x * 0.72));
                    shape = (1.0 - smoothstep(0.6, 0.75, er)) * smoothstep(0.18, 0.3, er);
                    nb = normalize(vec3f(cf * 2.2, x, 0.8));
                }
                case 4u: {   // chain: interlocking links straddling the ribbon
                    let link = abs(length(vec2f(cf * 1.7, x * 0.9)) - 0.45);
                    shape = 1.0 - smoothstep(0.1, 0.2, link);
                    nb = normalize(vec3f(cf * 2.0, x, 1.1));
                }
                case 5u: { shape = 0.0; }              // plain: bare ribbon
                default: {   // beads: round cabochons
                    let br = length(vec2f(cf * 1.9, x * 0.72));
                    shape = 1.0 - smoothstep(0.62, 0.78, br);
                    nb = normalize(vec3f(cf * 2.2, x, sqrt(max(1e-4, 1.0 - min(br / 0.7, 1.0) * min(br / 0.7, 1.0))) * 1.3));
                }
            }
            if p.veins > 0.001 {
                // Venation running over the surface of the ribbon.
                let vn = veins(q * 3.0 + vec2f(k), seed);
                ribbon = mix(ribbon, ribbon * (0.45 + 0.9 * vn)
                             + palette(p.hue + 0.6) * vn * 0.35, p.veins);
            }
            let bead = shape * p.jewels * fine;
            if bead > 0.002 {
                let gemcol = gem * (0.18 + 0.9 * max(dot(nb, light), 0.0))
                           + vec3f(1.0) * pow(max(dot(nb, half_vec), 0.0), 60.0);
                ribbon = mix(ribbon, gemcol, bead);
            }

            // Finer levels sit on top of coarser ones: depth reads as nested ornament.
            // A level thinner than a pixel fades out instead of dissolving into noise,
            // which keeps the coarse ribbons readable under the fine ones.
            let resolvable = smoothstep(1.2, 3.0, pitch / aa);
            col = mix(col, ribbon, cover * (0.55 + 0.45 * h) * resolvable);
        }

        // Rim light just off the ribbon edge keeps the filigree glowing against the ground.
        col += exp(-abs(abs(d) - w * 1.6) / max(w * 0.7, aa))
             * mix(vec3f(0.22, 0.5, 0.9), vec3f(0.9, 0.55, 0.18), p.style)
             * p.glow * 0.06;
    }

    col += haze * mix(vec3f(0.12, 0.24, 0.45), vec3f(0.35, 0.25, 0.12), p.style) * p.glow * 0.5;
    col *= 1.0 - 0.28 * dot(uv - 0.5, uv - 0.5);
    col = pow(max(col, vec3f(0.0)), vec3f(0.85));
    return vec4f(col, 1.0);
}

