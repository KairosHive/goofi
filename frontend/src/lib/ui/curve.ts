/** An easing curve as the widgets draw and read it: a preset's closed form, or a cubic bezier
 * from (0, 0) through two handles to (1, 1). `goofi_core::ease` is the owner; this mirrors it. */
import type { Curve } from '$lib/api/generated';

export type Handles = [number, number, number, number];
export type Preset = Exclude<Curve, { bezier: Handles }>;

export const PRESETS: Preset[] = ['step', 'linear', 'in', 'out', 'in_out', 'smooth'];

export const isBezier = (c: Curve): c is { bezier: Handles } => typeof c === 'object' && c !== null;

/** What a curve is called: the preset, or `bezier`. */
export const curveName = (c: Curve): string => (isBezier(c) ? 'bezier' : c);

/** The handles a custom curve starts from when it takes over from a preset: the preset's own
 * shape, so switching to custom changes nothing until a handle moves. */
export function handlesOf(c: Curve): Handles {
	if (isBezier(c)) return [...c.bezier] as Handles;
	switch (c) {
		case 'step':
			return [1, 0, 1, 0];
		case 'in':
			return [0.42, 0, 1, 1];
		case 'out':
			return [0, 0, 0.58, 1];
		case 'in_out':
			return [0.42, 0, 0.58, 1];
		case 'smooth':
			return [0.33, 0, 0.67, 1];
		default:
			return [0.33, 0.33, 0.67, 0.67];
	}
}

/** One coordinate of the cubic from 0 through `a` and `b` to 1, at parameter `t`. */
function cubic(a: number, b: number, t: number): number {
	const u = 1 - t;
	return 3 * u * u * t * a + 3 * u * t * t * b + t * t * t;
}

/** The parameter at which the curve's x reaches `x`: Newton from the diagonal, bisection where
 * the slope is too flat for it. */
function bezierT([x1, , x2]: Handles, x: number): number {
	let t = x;
	for (let i = 0; i < 8; i++) {
		const dx = cubic(x1, x2, t) - x;
		const u = 1 - t;
		const slope = 3 * u * u * x1 + 6 * u * t * (x2 - x1) + 3 * t * t * (1 - x2);
		if (Math.abs(dx) < 1e-7) return t;
		if (Math.abs(slope) < 1e-6) break;
		t = Math.min(1, Math.max(0, t - dx / slope));
	}
	let lo = 0;
	let hi = 1;
	for (let i = 0; i < 40; i++) {
		t = (lo + hi) / 2;
		if (cubic(x1, x2, t) < x) lo = t;
		else hi = t;
	}
	return t;
}

/** The blend at progress `x` in 0..1: 0 is where the move began, 1 is the target. */
export function curveAt(c: Curve, x: number): number {
	x = Math.min(1, Math.max(0, x));
	if (isBezier(c)) {
		const h = c.bezier;
		return cubic(h[1], h[3], bezierT(h, x));
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
		case 'smooth':
			return x * x * (3 - 2 * x);
		default:
			return x;
	}
}

/** The curve as an SVG path in a `w × h` box, y up: `n` samples, a step drawn as its two levels. */
export function curvePath(c: Curve, w: number, h: number, n = 48): string {
	if (c === 'step') return `M0 ${h} L${w} ${h} L${w} 0`;
	const pts: string[] = [];
	for (let i = 0; i <= n; i++) {
		const x = i / n;
		pts.push(`${(x * w).toFixed(2)} ${((1 - curveAt(c, x)) * h).toFixed(2)}`);
	}
	return `M${pts.join(' L')}`;
}
