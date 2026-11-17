/** An easing curve as the widgets draw and read it: a preset's closed form, or a line of anchor
 * points from (0, 0) to (1, 1), each joined to the next by a cubic bezier. `goofi_core::ease` is
 * the owner; this mirrors it. */
import type { Curve } from '$lib/api/generated';

/** An anchor point: where it is, and its two handles as offsets from it — `[x, y, in_dx, in_dy,
 * out_dx, out_dy]`. */
export type Point = [number, number, number, number, number, number];
export type Preset = Exclude<Curve, { custom: Point[] }>;

export const PRESETS: Preset[] = ['step', 'linear', 'in', 'out', 'in_out'];
/** What a curve is called where there is no room: three letters. */
export const SHORT: Record<string, string> = { step: 'stp', linear: 'lin', in: 'ein', out: 'eou', in_out: 'eio', custom: 'cst' };

export const isCustom = (c: Curve): c is { custom: Point[] } => typeof c === 'object' && c !== null;

/** What a curve is called: the preset, or `custom`. */
export const curveName = (c: Curve): string => (isCustom(c) ? 'custom' : c);

/** A preset as the two handles of one bezier from (0, 0) to (1, 1), as CSS writes one. */
const HANDLES: Record<Preset, [number, number, number, number]> = {
	step: [1, 0, 1, 0],
	linear: [0.33, 0.33, 0.67, 0.67],
	in: [0.42, 0, 1, 1],
	out: [0, 0, 0.58, 1],
	in_out: [0.42, 0, 0.58, 1]
};

/** The anchor points a curve is edited by: its own, or a preset's shape as two of them, so an
 * edit of a preset changes nothing until a point moves. */
export function pointsOf(c: Curve): Point[] {
	if (isCustom(c)) return c.custom.map((p) => [...p] as Point);
	const [x1, y1, x2, y2] = HANDLES[c];
	return [
		[0, 0, 0, 0, x1, y1],
		[1, 1, x2 - 1, y2 - 1, 0, 0]
	];
}

/** The points with every handle's x inside the segment it shapes: what keeps the curve from
 * folding back on itself. */
export function fitted(points: Point[]): Point[] {
	return points.map((p, i) => {
		const [before, after] = [points[i - 1], points[i + 1]];
		const within = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));
		return [p[0], p[1], before ? within(p[2], before[0] - p[0], 0) : 0, before ? p[3] : 0, after ? within(p[4], 0, after[0] - p[0]) : 0, after ? p[5] : 0];
	});
}

/** One coordinate of the cubic bezier through the four values, at parameter `t`. */
function cubic(a: number, b: number, c: number, d: number, t: number): number {
	const u = 1 - t;
	return u * u * u * a + 3 * u * u * t * b + 3 * u * t * t * c + t * t * t * d;
}

/** The blend at progress `x` in 0..1: 0 is where the move began, 1 is the target. */
export function curveAt(c: Curve, x: number): number {
	x = Math.min(1, Math.max(0, x));
	if (isCustom(c)) {
		const i = c.custom.findIndex((p, k) => k > 0 && x <= p[0]);
		if (i < 1) return x;
		const [a, b] = [c.custom[i - 1], c.custom[i]];
		// The handles' x inside the segment keep x(t) monotonic, so a bisection finds the one t.
		let [lo, hi] = [0, 1];
		for (let n = 0; n < 48; n++) {
			const t = (lo + hi) / 2;
			if (cubic(a[0], a[0] + a[4], b[0] + b[2], b[0], t) < x) lo = t;
			else hi = t;
		}
		return cubic(a[1], a[1] + a[5], b[1] + b[3], b[1], (lo + hi) / 2);
	}
	switch (c) {
		case 'step':
			return x >= 1 ? 1 : 0;
		case 'in':
			return x * x;
		case 'out':
			return 1 - (1 - x) * (1 - x);
		case 'in_out':
			return x < 0.5 ? 2 * x * x : 1 - 2 * (1 - x) * (1 - x);
		default:
			return x;
	}
}

/** The curve as an SVG path in a `w × h` box, y up: a custom curve as its own beziers, a preset
 * as `n` samples, a step as its two levels. */
export function curvePath(c: Curve, w: number, h: number, n = 48): string {
	if (c === 'step') return `M0 ${h} L${w} ${h} L${w} 0`;
	const at = (x: number, y: number): string => `${Number((x * w).toFixed(2))} ${Number(((1 - y) * h).toFixed(2))}`;
	if (isCustom(c)) return c.custom.map((p, i) => (i === 0 ? `M${at(p[0], p[1])}` : `C${at(c.custom[i - 1][0] + c.custom[i - 1][4], c.custom[i - 1][1] + c.custom[i - 1][5])} ${at(p[0] + p[2], p[1] + p[3])} ${at(p[0], p[1])}`)).join(' ');
	return `M${Array.from({ length: n + 1 }, (_, i) => at(i / n, curveAt(c, i / n))).join(' L')}`;
}
