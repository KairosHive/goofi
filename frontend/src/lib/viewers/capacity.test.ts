import { describe, it, expect } from 'vitest';
import { viewSpecForKind, viewSpecsForKind, CAP_FLOOR } from './capacity';

describe('viewSpecForKind', () => {
	it('line → the array it DRAWS (≤2-D), channels capped at what a plot can tell apart, samples enveloped to width', () => {
		expect(viewSpecForKind('line', 1600, 300)).toEqual({
			dtype: 'array',
			ndim: [['le', 2]],
			dims: [],
			reduce: [
				{ dim: 0, max: 32, method: 'subsample' },
				{ dim: -1, max: 1600, method: 'envelope' }
			],
			depth: 'f16'
		});
	});

	it('a kind describes what it cannot draw with an area preview, never its drawing axes', () => {
		// A line viewer parked on an image slot renders nothing; asking for its line axes there
		// would take the whole frame off a producer for a panel that only prints the shape.
		const specs = viewSpecsForKind('line', 800, 600);
		expect(specs).toHaveLength(2);
		expect(specs[1]).toEqual({
			dtype: 'array',
			ndim: [
				['ge', 3],
				['le', 3]
			],
			dims: [],
			reduce: [
				{ dim: 0, max: 600, method: 'area' },
				{ dim: 1, max: 800, method: 'area' }
			],
			depth: 'u8'
		});
		// The two never overlap, so one frame is only ever admitted by one of them.
		expect(specs[0].ndim).toEqual([['le', 2]]);
	});

	it('a kind that draws everything it accepts declares once', () => {
		expect(viewSpecsForKind('image', 640, 480)).toEqual([viewSpecForKind('image', 640, 480)]);
		expect(viewSpecsForKind('topomap', 100, 100)).toHaveLength(1);
	});

	it('image → array 2-D..3-D, area on both pixel axes', () => {
		expect(viewSpecForKind('image', 1280, 720)).toEqual({
			dtype: 'array',
			ndim: [
				['ge', 2],
				['le', 3]
			],
			dims: [],
			reduce: [
				{ dim: 0, max: 720, method: 'area' },
				{ dim: 1, max: 1280, method: 'area' }
			],
			depth: 'u8'
		});
	});

	// TrajectoryViewer reads a (dims × points) frame — shape[0] is the SIGNAL rows it pairs
	// i<j, shape[1] is the path length. So the axis worth reducing is the LAST one; capping
	// dim 0 would only drop signal rows while shipping every sample of the long axis.
	it('trajectory → array 2-D, subsample the point axis (the last one)', () => {
		expect(viewSpecForKind('trajectory', 800, 800)).toEqual({
			dtype: 'array',
			ndim: [['eq', 2]],
			dims: [],
			reduce: [{ dim: -1, max: 800, method: 'subsample' }]
		});
	});

	it('trajectory caps the point axis at MAX_POINTS on a very wide panel', () => {
		expect(viewSpecForKind('trajectory', 8000, 600).reduce).toEqual([
			{ dim: -1, max: 4096, method: 'subsample' }
		]);
	});

	it('topomap → array 1-D, no reduction', () => {
		expect(viewSpecForKind('topomap', 100, 100)).toEqual({
			dtype: 'array',
			ndim: [['eq', 1]],
			dims: [],
			reduce: []
		});
	});

	it('string / table → dtype-only compatibility, no array constraints or reduction', () => {
		expect(viewSpecForKind('string', 100, 100)).toEqual({ dtype: 'string', ndim: [], dims: [], reduce: [] });
		expect(viewSpecForKind('table', 100, 100)).toEqual({ dtype: 'table', ndim: [], dims: [], reduce: [] });
	});

	it('clamps degenerate (0-px / collapsed) sizes to the floor', () => {
		const spec = viewSpecForKind('line', 0, 0);
		expect(spec.reduce[0].max).toBe(32); // channel axis: the trace cap is below the floor
		expect(spec.reduce[1].max).toBe(CAP_FLOOR); // sample axis
	});
});
