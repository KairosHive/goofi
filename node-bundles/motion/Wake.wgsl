/* goofi
{ "doc": "a field that moving points stir, and that keeps developing without them\nWire `positions`, and `velocities` for the pull each point drags behind it: a body out of `PoseEstimation`, a flock out of `simulation:Swarm`, any frame shaped [n, 2] or wider. The points are a press on a medium that has a life of its own — activity competes across sizes, memory follows it slowly, and the field advects through its own memory gradient. Take the points away and it goes on developing. `touch` is what the points do, `structure` and `motion` are what the medium does on its own, and `tone` decides how it is drawn.",
  "tags": ["image", "motion", "simulation"],
  "state": ["field"],
  "inputs": [
    {"name": "positions", "kind": "ARRAY"},
    {"name": "velocities", "kind": "ARRAY"} ],
  "params": [
    {"group": "touch", "name": "reach", "kind": "float", "default": 0.06, "min": 0.002, "max": 0.4,
     "doc": "How far a point's press carries, as a share of the frame's height."},
    {"group": "touch", "name": "press", "kind": "float", "default": 0.9, "min": 0.0, "max": 4.0,
     "doc": "How hard a point pushes the field under it."},
    {"group": "touch", "name": "drag", "kind": "float", "default": 1.0, "min": 0.0, "max": 4.0,
     "doc": "How much a point's own velocity pulls the medium along with it."},
    {"group": "touch", "name": "fit", "kind": "bool", "default": false,
     "doc": "Stretch the coordinates onto the frame using the range they themselves span. Off keeps the unit box a camera and a swarm already use."},

    {"group": "structure", "name": "size", "kind": "int", "default": 3, "min": 1, "max": 24,
     "doc": "Radius of the smallest size that competes, in texels."},
    {"group": "structure", "name": "scales", "kind": "int", "default": 4, "min": 1, "max": 6,
     "doc": "How many sizes argue over each texel. Every one of them costs sixteen texture reads."},
    {"group": "structure", "name": "spacing", "kind": "float", "default": 1.9, "min": 1.1, "max": 3.0,
     "doc": "How far apart those sizes are: each is this much wider than the one below it."},
    {"group": "structure", "name": "inhibition", "kind": "float", "default": 2.1, "min": 1.1, "max": 4.0,
     "doc": "How much wider a surround is than the centre it argues with. This is what gives a domain a size of its own."},
    {"group": "structure", "name": "sensitivity", "kind": "float", "default": 0.16, "min": 0.01, "max": 1.0,
     "doc": "Where the drive stops growing. Small counts weak differences and makes fine structure."},

    {"group": "motion", "name": "pace", "kind": "float", "default": 0.5, "min": 0.0, "max": 2.0,
     "doc": "How much of a tick's drive the field actually takes."},
    {"group": "motion", "name": "flow", "kind": "float", "default": 1.1, "min": 0.0, "max": 4.0,
     "doc": "How far the field carries itself along its own gradient each tick."},
    {"group": "motion", "name": "curl", "kind": "float", "default": 0.45, "min": 0.0, "max": 1.0,
     "doc": "The turn of that flow: rotation at 0, convergence at 1, and every mixture between."},
    {"group": "motion", "name": "memory", "kind": "float", "default": 0.97, "min": 0.5, "max": 0.999,
     "doc": "How slowly memory follows activity. The gap between the two is what stops the field settling."},
    {"group": "motion", "name": "persistence", "kind": "float", "default": 0.55, "min": -1.0, "max": 1.0,
     "doc": "Above zero a departure from memory sustains itself; below zero it is pulled back."},
    {"group": "motion", "name": "balance", "kind": "float", "default": 0.06, "min": 0.0, "max": 0.5,
     "doc": "A weak pull toward mid level, so the field neither saturates nor dies out."},

    {"group": "seed", "name": "seed", "kind": "int", "default": 1, "min": 0, "max": 100000,
     "doc": "Which field the first tick starts from."},
    {"group": "seed", "name": "novelty", "kind": "float", "default": 0.0, "min": 0.0, "max": 0.2,
     "doc": "Fresh noise at every tick. Zero is the field on its own."},

    {"group": "tone", "name": "exposure", "kind": "float", "default": 1.6, "min": 0.1, "max": 6.0,
     "doc": "How much light the picture is given before the curve rolls it off."},
    {"group": "tone", "name": "gamma", "kind": "float", "default": 2.2, "min": 1.0, "max": 3.0},
    {"group": "tone", "name": "filigree", "kind": "float", "default": 0.8, "min": 0.0, "max": 3.0,
     "doc": "How brightly the edges between domains are drawn."},
    {"group": "tone", "name": "warmth", "kind": "float", "default": 0.9, "min": 0.0, "max": 3.0,
     "doc": "How much colour goes where the field is changing rather than where it is high."},
    {"group": "tone", "name": "glow", "kind": "float", "default": 0.7, "min": 0.0, "max": 3.0,
     "doc": "How much a domain is lit by the size that won it, and by the points that passed through."},
    {"group": "tone", "name": "contours", "kind": "float", "default": 5.0, "min": 0.0, "max": 24.0,
     "doc": "Bands of equal level drawn across the field. Zero draws none."},
    {"group": "tone", "name": "sharpness", "kind": "float", "default": 6.0, "min": 1.0, "max": 24.0,
     "doc": "How thin those bands are."} ] }
*/

const TAU: f32 = 6.283185307;
const QUARTER: f32 = 1.570796327;

// Every texel tests every point, so this is the multiplier the whole cost model turns on: a frame
// with more rows than this is walked at a stride and the rest are not read.
const MAX_POINTS: i32 = 128;

// The gradient of a settled memory channel is a hundredth of a level across one texel, and an
// advection has to move whole texels to be an advection. This is the one place a bare number
// stands in for a unit conversion, so `flow` can read as "texels a tick" at 1.
const CARRY: f32 = 48.0;

// A point's velocity arrives in frame-widths a second, and a hand crosses a frame in about two
// seconds. This puts `drag` at 1 near the medium's own speed rather than a hundred times past it.
const PULL: f32 = 0.03;

const DEEP: vec3f = vec3f(0.04, 0.05, 0.10);
const BODY: vec3f = vec3f(0.20, 0.42, 0.62);
const LIGHT: vec3f = vec3f(0.92, 0.88, 0.78);
const EDGE: vec3f = vec3f(0.45, 0.65, 0.95);
const WARM: vec3f = vec3f(1.00, 0.52, 0.24);
const GLOW: vec3f = vec3f(0.62, 0.35, 0.95);
const BAND: vec3f = vec3f(0.85, 0.90, 1.00);

/// No edges: structure that leaves one side arrives at the other.
fn wrap(at: vec2i) -> vec2i {
    let size = vec2i(textureDimensions(field));
    return ((at % size) + size) % size;
}

fn activity(at: vec2i) -> f32 {
    return textureLoad(field, wrap(at), 0).r;
}

fn remembered(at: vec2i) -> f32 {
    return textureLoad(field, wrap(at), 0).g;
}

/// The mean of eight neighbours at radius `r` — axials and diagonals, so no direction is favoured
/// and structure does not line up with the pixel grid.
fn ring(at: vec2i, r: i32) -> f32 {
    let d = max(r, 1);
    let e = max(i32(round(f32(d) * 0.70710678)), 1);
    var s = activity(at + vec2i(d, 0)) + activity(at + vec2i(-d, 0));
    s += activity(at + vec2i(0, d)) + activity(at + vec2i(0, -d));
    s += activity(at + vec2i(e, e)) + activity(at + vec2i(-e, e));
    s += activity(at + vec2i(e, -e)) + activity(at + vec2i(-e, -e));
    return s * 0.125;
}

/// The field part way between texels. Nearest-neighbour here quantizes the flow into blocky
/// jitter, which reads as boiling rather than as motion.
fn bilinear(at: vec2f) -> vec4f {
    let base = floor(at);
    let f = at - base;
    let i = vec2i(base);
    let a = textureLoad(field, wrap(i), 0);
    let b = textureLoad(field, wrap(i + vec2i(1, 0)), 0);
    let c = textureLoad(field, wrap(i + vec2i(0, 1)), 0);
    let d = textureLoad(field, wrap(i + vec2i(1, 1)), 0);
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

fn hashed(at: vec2i, salt: u32) -> f32 {
    var h = u32(at.x) * 374761393u + u32(at.y) * 668265263u + salt * 2246822519u;
    h = (h ^ (h >> 13u)) * 1274126177u;
    return f32(h ^ (h >> 16u)) * 2.3283064e-10;
}

/// Whether a frame arrived at all: an unwired input is the one shared transparent texel.
fn wired(dims: vec2i, alpha: f32) -> bool {
    return dims.x > 1 || dims.y > 1 || alpha >= 0.5;
}

/// Column `k` of row `i`, in the box the picture is drawn in.
fn coord(i: i32, k: i32) -> f32 {
    let v = textureLoad(positions, vec2i(k, i), 0).r;
    if p.fit == 0u {
        return v;
    }
    return (v - p.positions_lo) / max(p.positions_hi - p.positions_lo, 1e-6);
}

/// Where row `i` is going, in the same box. A row past the end of the velocity frame is still.
fn drift(i: i32, dims: vec2i) -> vec2f {
    if i >= dims.y {
        return vec2f(0.0);
    }
    let x = textureLoad(velocities, vec2i(0, i), 0).r;
    let y = select(0.0, textureLoad(velocities, vec2i(1, i), 0).r, dims.x > 1);
    return vec2f(x, y);
}

/// What the points do to this texel: how hard they press on it, and which way they pull it.
fn touch(uv: vec2f) -> vec3f {
    let dims = vec2i(textureDimensions(positions));
    if !wired(dims, textureLoad(positions, vec2i(0, 0), 0).a) || dims.x < 1 {
        return vec3f(0.0);
    }
    let moving = vec2i(textureDimensions(velocities));
    let carries = wired(moving, textureLoad(velocities, vec2i(0, 0), 0).a);
    let stride = max(1, dims.y / MAX_POINTS);
    let aspect = resolution.x / max(resolution.y, 1.0);
    let reach = max(p.reach, 1e-4);

    var press = 0.0;
    var pull = vec2f(0.0);
    for (var i = 0; i < dims.y; i = i + stride) {
        // One read decides whether the rest of the row is worth reading: a point this far off in x
        // cannot touch this texel however near it is in y. Most texels are far from most points,
        // so this is what keeps the loop at about one read a point.
        let dx = (coord(i, 0) - uv.x) * aspect;
        if abs(dx) >= reach {
            continue;
        }
        let dy = select(0.0, coord(i, 1) - uv.y, dims.x > 1);
        let r = length(vec2f(dx, dy)) / reach;
        if r >= 1.0 {
            continue;
        }
        let w = (1.0 - r * r) * (1.0 - r * r);
        press += w;
        if carries {
            pull += drift(i, moving) * w;
        }
    }
    return vec3f(press, pull.x, pull.y);
}

fn next_field(uv: vec2f) -> vec4f {
    let at = vec2i(uv * resolution);
    if frame == 0u {
        let start = hashed(at, u32(p.seed));
        return vec4f(start, start, 0.0, 0.0);
    }

    let t = touch(uv);

    // The medium carries itself: a velocity taken from its own slow channel, turned between
    // rotation and convergence, with whatever the points are dragging added to it.
    let g = vec2f(
        remembered(at + vec2i(1, 0)) - remembered(at - vec2i(1, 0)),
        remembered(at + vec2i(0, 1)) - remembered(at - vec2i(0, 1)),
    );
    let a = p.curl * QUARTER;
    var vel = (vec2f(-g.y, g.x) * cos(a) + g * sin(a)) * CARRY;
    vel += vec2f(t.y, t.z) * (p.drag * PULL * resolution.y);
    let old = bilinear(vec2f(at) + vec2f(0.5) - vel * p.flow);

    // One rule at several sizes, and the size nearest balance rules this texel. Taking the
    // STRONGEST instead lets one size win everywhere; nearest balance lets domains of different
    // sizes coexist and fight over territory.
    let count = max(p.scales, 1);
    var best = 1e9;
    var drive = 0.0;
    var won = 0.0;
    for (var k = 0; k < count; k = k + 1) {
        let r = i32(round(f32(p.size) * pow(p.spacing, f32(k))));
        let delta = ring(at, r) - ring(at, i32(round(f32(r) * p.inhibition)));
        if abs(delta) < best {
            best = abs(delta);
            drive = delta;
            won = f32(k) / max(f32(count - 1), 1.0);
        }
    }

    let fatigue = old.r - old.g;
    let raw = drive + p.persistence * fatigue + p.balance * (0.5 - old.r) + p.press * t.x
        + p.novelty * (hashed(at, frame + u32(p.seed)) - 0.5);
    // A ratio has gain near zero and an asymptote far from it. A clamp here would make regions
    // where the derivative is zero, and the field freezes in every one of them.
    let push = raw / (p.sensitivity + abs(raw));

    let value = clamp(old.r + p.pace * push, 0.0, 1.0);
    let memory = mix(value, old.g, p.memory);
    let provenance = mix(old.b, won, 0.035);
    let stirred = mix(old.a, min(length(vec2f(t.y, t.z)), 1.0), 0.08);
    return vec4f(value, memory, provenance, stirred);
}

fn shade(uv: vec2f) -> vec4f {
    let at = vec2i(uv * resolution);
    let c = textureLoad(field, wrap(at), 0);
    let edge = abs(activity(at + vec2i(1, 0)) - activity(at - vec2i(1, 0)))
        + abs(activity(at + vec2i(0, 1)) - activity(at - vec2i(0, 1)));
    let departure = abs(c.r - c.g);

    var col = mix(DEEP, BODY, smoothstep(0.15, 0.85, c.r));
    col = mix(col, LIGHT, smoothstep(0.65, 1.0, c.r));
    col += EDGE * (p.filigree * edge * 6.0);
    col += WARM * (p.warmth * departure * 4.0);
    col += GLOW * (p.glow * c.b * (0.25 + c.a));
    col += BAND * (pow(0.5 + 0.5 * cos(c.r * p.contours * TAU), p.sharpness) * 0.12);

    // Every texture here is Rgba16Float and these terms sum past 1, so the picture is tone-mapped
    // rather than clipped.
    col = 1.0 - exp(-col * p.exposure);
    return vec4f(pow(max(col, vec3f(0.0)), vec3f(1.0 / p.gamma)), 1.0);
}
