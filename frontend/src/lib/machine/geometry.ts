/** A transition's course between two state boxes, in FLOW units: a straight line from centre to
 * centre cut at each border, or a loop off the right side when a state re-enters itself. The edge
 * draws it and the playhead dots fly along it, so both agree on every point. */

export interface Box {
	x: number;
	y: number;
	w: number;
	h: number;
}

export interface Course {
	/** The SVG path. */
	d: string;
	length: number;
	/** The point `s` (0..1) of the way along, by arc length, and the heading there in degrees. */
	at(s: number): { x: number; y: number; angle: number };
	/** The `s` of the point nearest `p`. */
	near(p: { x: number; y: number }): number;
}

/** A loop's reach beyond its box. */
const LOOP_R = 22;

/** Where a ray from `(px, py)`, a point inside the box, along `(dx, dy)` leaves it. */
function exit(b: Box, px: number, py: number, dx: number, dy: number): { x: number; y: number } {
	const tx = dx > 0 ? (b.x + b.w - px) / dx : dx < 0 ? (b.x - px) / dx : Infinity;
	const ty = dy > 0 ? (b.y + b.h - py) / dy : dy < 0 ? (b.y - py) / dy : Infinity;
	const t = Math.min(tx, ty);
	return { x: px + dx * t, y: py + dy * t };
}

/** A polyline as a course: arc length is what `at` walks, so chevrons space evenly on a curve too. */
function polyline(pts: { x: number; y: number }[], d: string): Course {
	const cum = [0];
	for (let i = 1; i < pts.length; i++) cum.push(cum[i - 1] + Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y));
	const length = cum[cum.length - 1];
	return {
		d,
		length,
		near(p) {
			let best = { d2: Infinity, s: 0 };
			for (let i = 1; i < pts.length; i++) {
				const a = pts[i - 1];
				const b = pts[i];
				const seg = cum[i] - cum[i - 1];
				const f = seg === 0 ? 0 : Math.min(1, Math.max(0, ((p.x - a.x) * (b.x - a.x) + (p.y - a.y) * (b.y - a.y)) / (seg * seg)));
				const d2 = (a.x + (b.x - a.x) * f - p.x) ** 2 + (a.y + (b.y - a.y) * f - p.y) ** 2;
				if (d2 < best.d2) best = { d2, s: length === 0 ? 0 : (cum[i - 1] + seg * f) / length };
			}
			return best.s;
		},
		at(s) {
			const want = Math.min(Math.max(s, 0), 1) * length;
			let i = 1;
			while (i < cum.length - 1 && cum[i] < want) i++;
			const a = pts[i - 1];
			const b = pts[i];
			const seg = cum[i] - cum[i - 1];
			const f = seg === 0 ? 0 : (want - cum[i - 1]) / seg;
			return { x: a.x + (b.x - a.x) * f, y: a.y + (b.y - a.y) * f, angle: (Math.atan2(b.y - a.y, b.x - a.x) * 180) / Math.PI };
		}
	};
}

/** The course from `from` to `to`, shifted `offset` to its right so two transitions between one
 * pair of boxes run side by side; the same box twice is a loop. */
export function course(from: Box, to: Box, offset = 0): Course {
	if (from === to || (from.x === to.x && from.y === to.y && from.w === to.w && from.h === to.h)) return loop(from);
	const dx = to.x + to.w / 2 - (from.x + from.w / 2);
	const dy = to.y + to.h / 2 - (from.y + from.h / 2);
	if (dx === 0 && dy === 0) return loop(from);
	const len = Math.hypot(dx, dy);
	const ox = (-dy / len) * offset;
	const oy = (dx / len) * offset;
	const a = exit(from, from.x + from.w / 2 + ox, from.y + from.h / 2 + oy, dx, dy);
	const b = exit(to, to.x + to.w / 2 + ox, to.y + to.h / 2 + oy, -dx, -dy);
	return polyline([a, b], `M ${a.x} ${a.y} L ${b.x} ${b.y}`);
}

/** How far apart two courses between one pair of boxes run: a label's height and a little more,
 * so the labels of a pair clear each other on a short line too. */
export const LANE_GAP = 44;

/** A transition's lane among the `n` between its pair of boxes: how far off the centre line it
 * runs, in the pair's one frame so the two directions take opposite sides, and where between the
 * two CENTRES its label sits — the pair's frame again, so the labels of a pair never meet however
 * each course is cut by its box. */
export function lane(k: number, n: number, source: string, target: string): { offset: number; at: number } {
	const side = source <= target ? 1 : -1;
	const along = n === 1 ? 0.5 : 0.3 + (0.4 * k) / (n - 1);
	return { offset: side * (k - (n - 1) / 2) * LANE_GAP, at: side === 1 ? along : 1 - along };
}

/** Where a course's label sits: the course's point nearest to `at` of the way from `from`'s
 * centre to `to`'s, kept off both ends. */
export function labelAt(c: Course, from: Box, to: Box, at: number): { x: number; y: number } {
	const ref = { x: from.x + from.w / 2 + (to.x + to.w / 2 - from.x - from.w / 2) * at, y: from.y + from.h / 2 + (to.y + to.h / 2 - from.y - from.h / 2) * at };
	return c.at(Math.min(0.85, Math.max(0.15, c.near(ref))));
}

/** A loop out of the right side and back in, bulging past the box by LOOP_R. */
function loop(b: Box): Course {
	const x = b.x + b.w;
	const p0 = { x, y: b.y + b.h * 0.3 };
	const p3 = { x, y: b.y + b.h * 0.7 };
	const p1 = { x: x + 2.6 * LOOP_R, y: p0.y - 1.4 * LOOP_R };
	const p2 = { x: x + 2.6 * LOOP_R, y: p3.y + 1.4 * LOOP_R };
	const pts = [];
	for (let i = 0; i <= 32; i++) {
		const t = i / 32;
		const u = 1 - t;
		pts.push({
			x: u * u * u * p0.x + 3 * u * u * t * p1.x + 3 * u * t * t * p2.x + t * t * t * p3.x,
			y: u * u * u * p0.y + 3 * u * u * t * p1.y + 3 * u * t * t * p2.y + t * t * t * p3.y
		});
	}
	return polyline(pts, `M ${p0.x} ${p0.y} C ${p1.x} ${p1.y}, ${p2.x} ${p2.y}, ${p3.x} ${p3.y}`);
}

/** The course's path with its direction marks IN it: a chevron every `step`, clear of both ends,
 * as subpaths of the one stroke — so the line and its marks are one object to hover and select. */
export function marked(c: Course, step = 72, margin = 24, size = 5): string {
	let d = c.d;
	for (let along = step; along < c.length - margin; along += step) {
		const { x, y, angle } = c.at(along / c.length);
		const a = (angle * Math.PI) / 180;
		const arm = (sx: number, sy: number): string => `${x + sx * Math.cos(a) - sy * Math.sin(a)} ${y + sx * Math.sin(a) + sy * Math.cos(a)}`;
		d += ` M ${arm(-size, -size * 0.8)} L ${x} ${y} L ${arm(-size, size * 0.8)}`;
	}
	return d;
}
