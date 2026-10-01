/** Shared snap-to-edges/center/gap logic for the node drag and the placement preview. */
import { NODE } from './nodeMetrics';

export type Bounds = {
	left: number;
	right: number;
	top: number;
	bottom: number;
	cx: number;
	cy: number;
};

export type Guide = { x?: number; y?: number; opacity: number };

const SNAP_THRESHOLD = 15; // engage snap within this many flow-units
const SNAP_RANGE = 45; // start fading in guide hints from this far out
export const DEFAULT_NODE_H = 168; // a typical node: header + one open viewer (36 + 7u)

export function makeBounds(x: number, y: number, w: number, h: number): Bounds {
	return {
		left: x,
		right: x + w,
		top: y,
		bottom: y + h,
		cx: x + w / 2,
		cy: y + h / 2
	};
}

const GAPS = { x: [0, NODE.width * 0.25], y: [0, DEFAULT_NODE_H * 0.5] };
const AXES = ['y', 'x'] as const;

/** Every [mine, other] edge pair on one axis: equal edges, abutting edges at each gap, centres. */
function pairs(me: Bounds, oe: Bounds, axis: 'x' | 'y'): [number, number][] {
	const [lo, hi, c] = axis === 'x' ? (['left', 'right', 'cx'] as const) : (['top', 'bottom', 'cy'] as const);
	return GAPS[axis].flatMap((gap) => {
		const p: [number, number][] = [[me[lo], oe[lo]], [me[hi], oe[hi]], [me[lo], oe[hi] + gap], [me[hi], oe[lo] - gap]];
		return gap === 0 ? [...p, [me[c], oe[c]]] : p;
	});
}

export function computeSnapDelta(
	draggedBounds: Bounds[],
	targets: Bounds[],
	altKey: boolean
): { dx: number; dy: number; guides: Guide[] } {
	if (altKey || draggedBounds.length === 0 || targets.length === 0) {
		return { dx: 0, dy: 0, guides: [] };
	}

	const best = { x: { dist: Infinity, d: 0 }, y: { dist: Infinity, d: 0 } };
	for (const me of draggedBounds)
		for (const oe of targets)
			for (const axis of AXES)
				for (const [mine, other] of pairs(me, oe, axis)) {
					const d = Math.abs(mine - other);
					if (d < SNAP_THRESHOLD && d < best[axis].dist) best[axis] = { dist: d, d: other - mine };
				}
	const dx = best.x.d;
	const dy = best.y.d;

	const guides: Guide[] = [];
	for (const me of draggedBounds) {
		const shifted = makeBounds(me.left + dx, me.top + dy, me.right - me.left, me.bottom - me.top);
		for (const oe of targets)
			for (const axis of AXES)
				for (const [mine, other] of pairs(shifted, oe, axis)) {
					const d = Math.abs(mine - other);
					const opacity = d < 0.5 ? 1 : 1 - d / SNAP_RANGE;
					if (d < SNAP_RANGE) guides.push(axis === 'x' ? { x: other, opacity } : { y: other, opacity });
				}
	}

	return { dx, dy, guides };
}
