import type { Machine } from '$lib/api/generated';
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
	label?: { x: number; y: number };
	/** The point `s` (0..1) of the way along, by arc length, and the heading there in degrees. */
	at(s: number): { x: number; y: number; angle: number };
}

/** A loop's reach beyond its box. */
const LOOP_R = 22;
/** Bounds shared with the label's CSS, including touch padding. */
export const LABEL_W = 240;
export const LABEL_H = 44;
const LABEL_GAP = 8;
export interface LabelSize { w: number; h: number }

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
export function course(from: Box, to: Box, offset = 0, at = 0.5, loopIndex = 0, label?: { x: number; y: number }): Course {
	if (from === to || (from.x === to.x && from.y === to.y && from.w === to.w && from.h === to.h)) return loop(from, loopIndex, label);
	const ax = from.x + from.w / 2;
	const ay = from.y + from.h / 2;
	const bx = to.x + to.w / 2;
	const by = to.y + to.h / 2;
	const dx = bx - ax;
	const dy = by - ay;
	const len = Math.hypot(dx, dy);
	if (len === 0) return loop(from, loopIndex, label);
	const mid = label ?? { x: ax + dx * at - dy / len * offset, y: ay + dy * at + dx / len * offset };
	// Fan to separate lanes from ports on the border. No lane starts outside its card.
	const a = exit(from, ax, ay, mid.x - ax, mid.y - ay);
	const b = exit(to, bx, by, mid.x - bx, mid.y - by);
	return { ...polyline([a, mid, b], `M ${a.x} ${a.y} L ${mid.x} ${mid.y} L ${b.x} ${b.y}`), label: mid };
}

/** Expand authored wildcard sources as a one-way canvas projection. */
function routes(m: Machine): { key: string; id: string; source: string; target: string }[] {
	return Object.entries(m.transitions).flatMap(([id, t]) => (t.from === '*' ? Object.keys(m.states) : [t.from])
		.map((source) => ({ key: t.from === '*' ? `${id}@${source}` : id, id, source, target: t.to })));
}

export type RouteGeometry = ReturnType<typeof routes>[number] & { course: Course; label: { x: number; y: number } };

/** Follow the runtime endpoints, including when an authored transition changes during travel. */
export function flightCourse(geometry: Map<string, RouteGeometry>, id: string, source: string, target: string, box: (id: string) => Box | null): Course | null {
	const route = [...geometry.values()].find((r) => r.id === id && r.source === source && r.target === target);
	if (route) return route.course;
	const from = box(source);
	const to = source === target ? from : box(target);
	return from && to ? course(from, to) : null;
}

/** Resolve the whole scene once, in authored order, for edges, preview and dots. */
export function courses(m: Machine, box: (id: string) => Box | null, size: (key: string) => LabelSize | undefined = () => undefined): Map<string, RouteGeometry> {
	const projected = routes(m);
	const pairKey = (r: (typeof projected)[number]): string => [r.source, r.target].sort().join('|');
	const pairs = Map.groupBy(projected, pairKey);
	const result = new Map<string, RouteGeometry>();
	const obstacles = Object.keys(m.states).flatMap((id) => {
		const b = box(id);
		return b ? [b] : [];
	});
	for (const sibling of projected) {
		const { w, h } = size(sibling.key) ?? { w: LABEL_W, h: LABEL_H };
		const siblings = pairs.get(pairKey(sibling))!;
		const k = siblings.indexOf(sibling);
		const a = box(sibling.source);
		const b = sibling.source === sibling.target ? a : box(sibling.target);
		if (!a || !b) continue;
		const l = lane(k, siblings.length, sibling.source, sibling.target);
		let c = course(a, b, l.offset, l.at, k);
		const label = { ...(c.label ?? c.at(0.5)) };
		const dx = b.x + b.w / 2 - a.x - a.w / 2;
		const dy = b.y + b.h / 2 - a.y - a.h / 2;
		const len = Math.hypot(dx, dy);
		const side = l.offset < 0 ? -1 : 1;
		const normal = len === 0 ? { x: 1, y: 0 } : { x: -dy / len * side, y: dx / len * side };
		// A lane clears cards and all earlier labels, including other state pairs.
		let moved: boolean;
		do {
			moved = false;
			for (const obstacle of obstacles) {
				const expanded = { x: obstacle.x - w / 2 - LABEL_GAP, y: obstacle.y - h / 2 - LABEL_GAP,
					w: obstacle.w + w + 2 * LABEL_GAP, h: obstacle.h + h + 2 * LABEL_GAP };
				if (label.x < expanded.x || label.x > expanded.x + expanded.w || label.y < expanded.y || label.y > expanded.y + expanded.h) continue;
				const border = exit(expanded, label.x, label.y, normal.x, normal.y);
				label.x = border.x + normal.x;
				label.y = border.y + normal.y;
				moved = true;
			}
		} while (moved);
		c = course(a, b, l.offset, l.at, k, label);
		result.set(sibling.key, { ...sibling, course: c, label });
		obstacles.push({ x: label.x - w / 2, y: label.y - h / 2, w, h });
	}
	return result;
}

/** How far apart two courses between one pair of boxes run: a label's height and a little more,
 * so the labels of a pair clear each other on a short line too. */
export const LANE_GAP = 44;

/** A transition's lane among the `n` between its pair of boxes: how far off the centre line it
 * runs, in the pair's one frame so the two directions take opposite sides, and where between the
 * two CENTRES its label sits — the pair's frame again, so the labels of a pair never meet however
 * each course is cut by its box. */
function lane(k: number, n: number, source: string, target: string): { offset: number; at: number } {
	const side = source <= target ? 1 : -1;
	const along = n === 1 ? 0.5 : 0.3 + (0.4 * k) / (n - 1);
	return { offset: side * (k - (n - 1) / 2) * LANE_GAP, at: side === 1 ? along : 1 - along };
}

/** A loop out of the right side and back in, bulging past the box by LOOP_R. */
function loop(b: Box, index = 0, label?: { x: number; y: number }): Course {
	const radius = Math.max(LOOP_R + index * LANE_GAP, ((label?.x ?? b.x + b.w) - b.x - b.w) / 1.95);
	const x = b.x + b.w;
	const p0 = { x, y: b.y + b.h * 0.3 };
	const p3 = { x, y: b.y + b.h * 0.7 };
	const p1 = { x: x + 2.6 * radius, y: p0.y - 1.4 * radius };
	const p2 = { x: x + 2.6 * radius, y: p3.y + 1.4 * radius };
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
