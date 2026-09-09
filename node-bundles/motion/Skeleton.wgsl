/* goofi
{ "doc": "the figure a set of landmarks makes, drawn on clear ground\nWire `positions` from `PoseEstimation` and it draws what that mode found — a body's bones, a hand's fingers, a face's contours — because `rig` reads the shape from the ROW COUNT and needs telling nothing. Wire `velocities` too for a streak behind each point. Nothing is drawn on the ground, so `graphics:Composite` puts it straight over the picture it came from. A frame with no rig it knows, a swarm or a set of boxes, is drawn as its points alone.",
  "tags": ["image", "motion"],
  "inputs": [
    {"name": "positions", "kind": "ARRAY"},
    {"name": "velocities", "kind": "ARRAY"} ],
  "params": [
    {"group": "rig", "name": "rig", "kind": "str", "default": "auto",
     "options": ["auto", "none", "pose", "hand", "face", "holistic"],
     "doc": "Which figure the rows describe. `auto` reads it from how many rows there are, and counts the figures too."},
    {"group": "rig", "name": "fit", "kind": "bool", "default": false,
     "doc": "Stretch the coordinates onto the frame using the range they themselves span. Off keeps the unit box a landmark already uses."},

    {"group": "draw", "name": "bone", "kind": "float", "default": 2.0, "min": 0.0, "max": 12.0,
     "doc": "How thick a link between two landmarks is drawn, in texels. Zero draws none."},
    {"group": "draw", "name": "joint", "kind": "float", "default": 3.5, "min": 0.0, "max": 24.0,
     "doc": "How big a landmark itself is drawn, in texels. Zero draws none."},
    {"group": "draw", "name": "streak", "kind": "float", "default": 0.0, "min": 0.0, "max": 1.0,
     "doc": "How many seconds of travel to show as a tail behind each landmark. A velocity is per SECOND, so a tenth already draws a long one."},
    {"group": "draw", "name": "depth", "kind": "float", "default": 0.6, "min": 0.0, "max": 2.0,
     "doc": "How much the third column shrinks and dims what is further away."},

    {"group": "tone", "name": "colour", "kind": "str", "default": "part",
     "options": ["part", "speed", "depth", "plain"],
     "doc": "What decides a landmark's colour: which part of the figure it is, how fast it moves, how far away it is, or nothing."},
    {"group": "tone", "name": "hue", "kind": "float", "default": 0.55, "min": 0.0, "max": 1.0,
     "doc": "Where the colouring starts."},
    {"group": "tone", "name": "spread", "kind": "float", "default": 0.35, "min": 0.0, "max": 1.0,
     "doc": "How far it wanders from there."},
    {"group": "tone", "name": "glow", "kind": "float", "default": 1.0, "min": 0.0, "max": 4.0,
     "doc": "How brightly it is drawn. Past 1 it blooms in whatever it is composited into."} ] }
*/

const TAU: f32 = 6.283185307;

const POSE: i32 = 0;
const HAND: i32 = 1;
const FACE: i32 = 2;
const HOLISTIC: i32 = 3;

// How many landmarks each rig has, and where its links sit in EDGES.
const POSE_N: i32 = 33;
const HAND_N: i32 = 21;
const FACE_N: i32 = 478;
const POSE_AT: i32 = 0;
const POSE_LINKS: i32 = 35;
const HAND_AT: i32 = 35;
const HAND_LINKS: i32 = 21;
const FACE_AT: i32 = 56;
const FACE_LINKS: i32 = 124;

// The face's CONTOURS, not its tesselation: the full mesh is 2556 links, and every texel would
// walk every one of them. Eyes, brows, lips, nose and the oval are what carry the expression.
// A frame with more rows than this walks them at a stride, because the point loop is per texel.
const MAX_POINTS: i32 = 512;

// The longest tail a landmark may drag, in frame heights. A velocity is reported per SECOND and a
// hand crosses the frame in about one, so without this ceiling one landmark's streak is longer
// than the picture and a body's worth of them floods it to a flat wash.
const MAX_TAIL: f32 = 0.2;
const MAX_FIGURES: i32 = 8;

const EDGES: array<u32, 180> = array<u32, 180>(
    1u, 65538u, 131075u, 196615u, 4u, 262149u, 327686u, 393224u, 589834u, 720908u, 720909u, 851983u,
    983057u, 983059u, 983061u, 1114131u, 786446u, 917520u, 1048594u, 1048596u, 1048598u, 1179668u, 720919u,
    786456u, 1507352u, 1507353u, 1572890u, 1638427u, 1703964u, 1769501u, 1835038u, 1900575u, 1966112u,
    1769503u, 1835040u, 1u, 65541u, 589837u, 851985u, 327689u, 17u, 65538u, 131075u, 196612u, 327686u,
    393223u, 458760u, 589834u, 655371u, 720908u, 851982u, 917519u, 983056u, 1114130u, 1179667u, 1245204u,
    3997842u, 9568347u, 5963957u, 11862100u, 5505041u, 1114426u, 20578709u, 26542401u, 21037431u,
    24576291u, 3997881u, 12124200u, 2621479u, 2555941u, 2424832u, 267u, 17498381u, 17629454u, 17695129u,
    26804515u, 5111903u, 6226008u, 5767346u, 11665495u, 5701646u, 917821u, 20775314u, 26345790u, 20840772u,
    21233972u, 5111999u, 12517456u, 5242961u, 5308498u, 5373965u, 852280u, 20447543u, 20382006u, 20316575u,
    27197748u, 17236217u, 16318854u, 25559413u, 24445302u, 24510844u, 24904061u, 24969598u, 25035114u,
    17236434u, 30540164u, 25428355u, 25362818u, 25297281u, 25231744u, 25166222u, 26083690u, 18088219u,
    18546970u, 18481447u, 19333405u, 19661093u, 19202382u, 21889320u, 19398992u, 2162695u, 458915u,
    10682512u, 9437329u, 9502873u, 10027162u, 10092699u, 10158213u, 2162934u, 16122017u, 10551456u,
    10485919u, 10420382u, 10354845u, 10289325u, 11337861u, 3014709u, 3473460u, 3407937u, 4259895u,
    4587583u, 4128873u, 6881346u, 4325483u, 655698u, 22151465u, 19464524u, 21758236u, 18612475u, 16449925u,
    25493860u, 23331270u, 29753667u, 21168489u, 23658784u, 18874765u, 26018157u, 23921019u, 24838522u,
    24773008u, 26214777u, 24707224u, 9961620u, 9699504u, 11534485u, 9765014u, 9830536u, 8913068u,
    11272250u, 3801220u, 8650845u, 6095082u, 15335551u, 8323234u, 10616853u, 1376310u, 3539047u, 6750275u,
    4391021u, 7143434u);

/// One link, as the two rows it joins.
fn link(k: i32) -> vec2i {
    let e = EDGES[k];
    return vec2i(i32(e >> 16u), i32(e & 0xffffu));
}

fn rig_size(kind: i32) -> i32 {
    if kind == POSE { return POSE_N; }
    if kind == HAND { return HAND_N; }
    return FACE_N;
}

fn rig_at(kind: i32) -> i32 {
    if kind == POSE { return POSE_AT; }
    if kind == HAND { return HAND_AT; }
    return FACE_AT;
}

fn rig_links(kind: i32) -> i32 {
    if kind == POSE { return POSE_LINKS; }
    if kind == HAND { return HAND_LINKS; }
    return FACE_LINKS;
}

/// Whether a holistic frame carried a face, which is what moves the hands along it.
fn holistic_face(n: i32) -> i32 {
    return select(0, 1, n >= POSE_N + FACE_N);
}

/// Which figure the rows describe. The row COUNT is the only handle a shader has on a mode it
/// cannot see, and it is enough: no mode makes the same count as another.
fn rig_of(n: i32) -> i32 {
    if p.rig == 1u { return -1; }
    if p.rig == 2u { return POSE; }
    if p.rig == 3u { return HAND; }
    if p.rig == 4u { return FACE; }
    if p.rig == 5u { return HOLISTIC; }
    if n <= 0 { return -1; }
    if n % POSE_N == 0 { return POSE; }
    if n % HAND_N == 0 { return HAND; }
    if n % FACE_N == 0 { return FACE; }
    if n == 54 || n == 75 || n == 511 || n == 532 || n == 553 { return HOLISTIC; }
    return -1;
}

/// How many rigs are laid end to end in the frame.
fn figures(rig: i32, n: i32) -> i32 {
    if rig == HOLISTIC {
        let face = holistic_face(n);
        return 1 + face + (n - POSE_N - face * FACE_N) / HAND_N;
    }
    if rig < 0 { return 0; }
    return n / rig_size(rig);
}

/// Where figure `f` starts, and which rig it is. Holistic is one pose, then a face if it found
/// one, then a hand for each it found.
fn figure_at(rig: i32, n: i32, f: i32) -> vec2i {
    if rig != HOLISTIC { return vec2i(f * rig_size(rig), rig); }
    let face = holistic_face(n);
    if f == 0 { return vec2i(0, POSE); }
    if face == 1 && f == 1 { return vec2i(POSE_N, FACE); }
    return vec2i(POSE_N + face * FACE_N + (f - 1 - face) * HAND_N, HAND);
}

/// Column `k` of row `i`, in the box the picture is drawn in.
fn coord(i: i32, k: i32) -> f32 {
    let v = textureLoad(positions, vec2i(k, i), 0).r;
    if p.fit == 0u { return v; }
    return (v - p.positions_lo) / max(p.positions_hi - p.positions_lo, 1e-6);
}

fn depth_of(i: i32, wide: i32) -> f32 {
    return select(0.0, textureLoad(positions, vec2i(2, i), 0).r, wide > 2);
}

fn drift(i: i32, dims: vec2i) -> vec2f {
    if i >= dims.y || dims.x < 1 { return vec2f(0.0); }
    let x = textureLoad(velocities, vec2i(0, i), 0).r;
    let y = select(0.0, textureLoad(velocities, vec2i(1, i), 0).r, dims.x > 1);
    return vec2f(x, y);
}

/// Whether a frame arrived at all: an unwired input is the one shared transparent texel.
fn wired(dims: vec2i, alpha: f32) -> bool {
    return dims.x > 1 || dims.y > 1 || alpha >= 0.5;
}

fn hue(t: f32) -> vec3f {
    return 0.55 + 0.45 * cos(TAU * (fract(t) + vec3f(0.0, 0.33, 0.67)));
}

fn ink(under: vec4f, colour: vec3f, cover: f32) -> vec4f {
    return vec4f(mix(under.rgb, colour * p.glow, cover), max(under.a, cover));
}

/// How far `q` is from the segment `a`–`b`, all of them in units of the frame's height.
fn to_segment(q: vec2f, a: vec2f, b: vec2f) -> f32 {
    let ab = b - a;
    let t = clamp(dot(q - a, ab) / max(dot(ab, ab), 1e-9), 0.0, 1.0);
    return length(q - (a + ab * t));
}

/// What colours a row: which part of its own figure it is, how fast it goes, how far it is, or
/// nothing at all.
fn tint(within: i32, size: i32, z: f32, speed: f32) -> vec3f {
    var t = 0.0;
    if p.colour == 0u {
        t = f32(within) / f32(max(size, 1));
    } else if p.colour == 1u {
        t = clamp(speed, 0.0, 1.0);
    } else if p.colour == 2u {
        t = clamp(z * 0.5 + 0.5, 0.0, 1.0);
    }
    return hue(p.hue + p.spread * t);
}

fn shade(uv: vec2f) -> vec4f {
    let dims = vec2i(textureDimensions(positions));
    if !wired(dims, textureLoad(positions, vec2i(0, 0), 0).a) || dims.x < 1 {
        return vec4f(0.0);
    }
    let n = dims.y;
    let moving = vec2i(textureDimensions(velocities));
    let carries = wired(moving, textureLoad(velocities, vec2i(0, 0), 0).a);

    // Every distance is measured in frame HEIGHTS, so a joint is round rather than an ellipse on
    // a frame that is not square.
    let aspect = vec2f(resolution.x / max(resolution.y, 1.0), 1.0);
    let q = uv * aspect;
    let soft = 0.7 / max(resolution.y, 1.0);
    let bone = p.bone * 0.5 / max(resolution.y, 1.0);
    let joint = p.joint * 0.5 / max(resolution.y, 1.0);

    var out = vec4f(0.0);
    let rig = rig_of(n);

    if rig >= 0 && p.bone > 0.0 {
        let count = min(figures(rig, n), MAX_FIGURES);
        for (var f = 0; f < count; f = f + 1) {
            let fig = figure_at(rig, n, f);
            let kind = fig.y;
            let links = rig_links(kind);
            for (var k = 0; k < links; k = k + 1) {
                let e = link(rig_at(kind) + k);
                let ia = fig.x + e.x;
                let ib = fig.x + e.y;
                if ia >= n || ib >= n { continue; }
                // Two reads decide whether the other two are worth making: a link entirely to the
                // left or right of this texel cannot cover it, and most links are.
                let ax = coord(ia, 0) * aspect.x;
                let bx = coord(ib, 0) * aspect.x;
                if q.x < min(ax, bx) - bone - soft || q.x > max(ax, bx) + bone + soft { continue; }
                let a = vec2f(ax, coord(ia, 1));
                let b = vec2f(bx, coord(ib, 1));
                let z = (depth_of(ia, dims.x) + depth_of(ib, dims.x)) * 0.5;
                let near = clamp(1.0 - p.depth * z, 0.15, 2.0);
                let cover = 1.0 - smoothstep(bone * near - soft, bone * near + soft, to_segment(q, a, b));
                if cover > 0.0 {
                    let speed = length(drift(ia, moving)) * f32(carries);
                    out = ink(out, tint(e.x, rig_size(kind), z, speed) * near, cover);
                }
            }
        }
    }

    if p.joint > 0.0 || p.streak > 0.0 {
        let size = select(n, rig_size(rig), rig >= 0);
        let stride = max(1, n / MAX_POINTS);
        for (var i = 0; i < n; i = i + stride) {
            let px = coord(i, 0) * aspect.x;
            let travel = drift(i, moving) * (p.streak * f32(carries));
            let far = length(travel);
            let tail = select(travel, travel * (MAX_TAIL / max(far, 1e-6)), far > MAX_TAIL);
            let reach = joint + soft + length(tail);
            if abs(q.x - px) > reach { continue; }
            let here = vec2f(px, coord(i, 1));
            let z = depth_of(i, dims.x);
            let near = clamp(1.0 - p.depth * z, 0.15, 2.0);
            let colour = tint(i % max(size, 1), size, z, length(drift(i, moving)));
            let back = here - vec2f(tail.x * aspect.x, tail.y);
            if p.streak > 0.0 {
                let along = to_segment(q, here, back);
                let cover = (1.0 - smoothstep(0.0, joint * near + soft, along)) * 0.5;
                if cover > 0.0 { out = ink(out, colour * near * 0.6, cover); }
            }
            if p.joint > 0.0 {
                let cover = 1.0 - smoothstep(joint * near - soft, joint * near + soft, length(q - here));
                if cover > 0.0 { out = ink(out, colour * near, cover); }
            }
        }
    }
    return out;
}
