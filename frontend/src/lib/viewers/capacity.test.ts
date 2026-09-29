import { describe, it, expect } from 'vitest';
import { viewSpecForKind, viewSpecsForKind, CAP_FLOOR } from './capacity';

const axes = (kind: Parameters<typeof viewSpecForKind>[0], w: number, h: number, settings = {}) =>
	viewSpecForKind(kind, w, h, settings).reduce.map((r) => [r.dim, r.max]);

describe('viewSpecForKind', () => {
	it('a line caps its channels at what a plot tells apart and its samples at the width', () => {
		expect(axes('line', 1600, 300)).toEqual([
			[0, 32],
			[-1, 1600]
		]);
		expect(viewSpecForKind('line', 1600, 300).aspect, 'rows and samples are independent caps').toBeUndefined();
	});

	it('an image fits both pixel axes into its box, as one box', () => {
		expect(axes('image', 1280, 720)).toEqual([
			[0, 720],
			[1, 1280]
		]);
		expect(viewSpecForKind('image', 1280, 720).aspect).toBe(true);
	});

	it('a trajectory subsamples its point axis, the last one, up to a cap', () => {
		expect(axes('line', 800, 800, { mode: 'trajectory' })).toEqual([[-1, 800]]);
		expect(axes('line', 8000, 600, { mode: 'trajectory' })).toEqual([[-1, 4096]]);
	});

	it('a brain, a string and a table are served whole', () => {
		for (const kind of ['brain', 'string', 'table'] as const) expect(axes(kind, 100, 100)).toEqual([]);
	});

	it('a line parked on an image slot previews it as a box, and the two specs never overlap', () => {
		// Its line axes asked of an image would take the whole frame off a producer for a shape print.
		const [draws, preview] = viewSpecsForKind('line', 800, 600);
		expect(draws.ndim).toEqual([['le', 2]]);
		expect(preview.ndim).toEqual([
			['ge', 3],
			['le', 3]
		]);
		expect(preview.aspect).toBe(true);
		expect(viewSpecsForKind('image', 640, 480)).toEqual([viewSpecForKind('image', 640, 480)]);
	});

	it('clamps degenerate (0-px / collapsed) sizes to the floor', () => {
		const spec = viewSpecForKind('line', 0, 0);
		expect(spec.reduce[0].max).toBe(32); // channel axis: the trace cap is below the floor
		expect(spec.reduce[1].max).toBe(CAP_FLOOR);
	});
});
