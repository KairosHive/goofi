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
}

/** A loop's reach beyond its box. */
const LOOP_R = 22;

/** Where a ray from the box's centre along `(dx, dy)` leaves it. */
function exit(b: Box, dx: number, dy: number): { x: number; y: number } {
	const cx = b.x + b.w / 2;
	const cy = b.y + b.h / 2;
	const t = Math.min(dx === 0 ? Infinity : b.w / 2 / Math.abs(dx), dy === 0 ? Infinity : b.h / 2 / Math.abs(dy));
	return { x: cx + dx * t, y: cy + dy * t };
}

/** A polyline as a course: arc length is what `at` walks, so chevrons space evenly on a curve too. */
function polyline(pts: { x: number; y: number }[], d: string): Course {
	const cum = [0];
	for (let i = 1; i < pts.length; i++) cum.push(cum[i - 1] + Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y));
	const length = cum[cum.length - 1];
	return {
		d,
		length,
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

/** The course from `from` to `to`; the same box twice is a loop. */
export function course(from: Box, to: Box): Course {
	if (from === to || (from.x === to.x && from.y === to.y && from.w === to.w && from.h === to.h)) return loop(from);
	const dx = to.x + to.w / 2 - (from.x + from.w / 2);
	const dy = to.y + to.h / 2 - (from.y + from.h / 2);
	if (dx === 0 && dy === 0) return loop(from);
	const a = exit(from, dx, dy);
	const b = exit(to, -dx, -dy);
	return polyline([a, b], `M ${a.x} ${a.y} L ${b.x} ${b.y}`);
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
