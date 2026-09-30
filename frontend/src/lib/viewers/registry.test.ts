import { describe, it, expect } from 'vitest';
import { VIEWER_KINDS, type ViewerKind } from '$lib/api/vocab';
import { MODULES, viewSpecForKind, viewSpecsForKind, CAP_FLOOR, resolveKind, isRenderable, pinnedKind, resolveSettings } from './registry';
import type { DimCmp, SettingsMap } from './module';

const axes = (kind: ViewerKind, w: number, h: number, settings = {}) =>
	viewSpecForKind(kind, w, h, settings).reduce.map((r) => [r.dim, r.max]);

describe('viewSpecForKind', () => {
	it('a line caps its channels at what a plot tells apart and its samples at the width', () => {
		expect(axes('line', 1600, 300)).toEqual([
			[0, 32],
			[-1, 1600]
		]);
		expect(viewSpecForKind('line', 1600, 300).aspect, 'rows and samples are independent caps').toBeUndefined();
	});

	it('an image fits both pixel axes into its box, as one box, and takes its channels whole', () => {
		expect(axes('image', 1280, 720)).toEqual([
			[0, 720],
			[1, 1280],
			[2, 'whole']
		]);
		expect(viewSpecForKind('image', 1280, 720).aspect).toBe(true);
	});

	it('a trajectory subsamples its point axis, the last one, up to a cap, and keeps every row', () => {
		expect(axes('line', 800, 800, { mode: 'trajectory' })).toEqual([
			[0, 'whole'],
			[-1, 800]
		]);
		expect(axes('line', 8000, 600, { mode: 'trajectory' })[1]).toEqual([-1, 4096]);
	});

	it('a brain is served whole on every axis it draws; a string and a table ask nothing', () => {
		expect(axes('brain', 100, 100)).toEqual([
			[0, 'whole'],
			[1, 'whole']
		]);
		for (const kind of ['string', 'table'] as const) expect(axes(kind, 100, 100)).toEqual([]);
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

/** `actual <op> n`, as the fold reads a spec's `ndim`. */
const holds = (cmp: DimCmp, actual: number, n: number) =>
	({ lt: actual < n, le: actual <= n, eq: actual === n, ge: actual >= n, gt: actual > n })[cmp];

describe('every kind states an ask for every dim it draws', () => {
	// The fold reads nothing into a dim a viewer leaves unnamed: a kind that forgot one would be
	// served the fold of whoever else is on the slot, and draw a picture it never asked for.
	for (const k of VIEWER_KINDS) {
		if (!k.draws) continue;
		const m = MODULES[k.id];
		// The defaults, and each choice of each menu on its own: what a mode switch changes.
		const settings: SettingsMap[] = [{}];
		for (const s of m.settings) for (const o of s.options ?? []) settings.push({ [s.key]: o });
		for (const s of settings) {
			const spec = viewSpecForKind(k.id, 640, 480, s);
			for (let ndim = k.draws[0]; ndim <= k.draws[1]; ndim++) {
				if (!spec.ndim.every(([cmp, n]) => holds(cmp, ndim, n))) continue;
				const named = new Set(spec.reduce.map((r) => (r.dim < 0 ? ndim + r.dim : r.dim)));
				for (let d = 0; d < ndim; d++) {
					it(`${k.id} under ${JSON.stringify(s)} names dim ${d} of ${ndim}`, () => {
						expect(named.has(d)).toBe(true);
					});
				}
			}
		}
	}
});

describe('resolveKind', () => {
	it('forces string/table viewers by dtype regardless of stored kind', () => {
		expect(resolveKind('STRING', 'image')).toBe('string');
		expect(resolveKind('TABLE', 'line')).toBe('table');
	});
	it('uses the stored kind for ARRAY, falling back to line', () => {
		expect(resolveKind('ARRAY', 'image')).toBe('image');
		expect(resolveKind('ARRAY', undefined)).toBe('line');
	});
	it('falls back to line for null dtype', () => {
		expect(resolveKind(null, undefined)).toBe('line');
	});
	it('opens a slot nobody has chosen for with what draws its dtype', () => {
		expect(resolveKind('TEXTURE', undefined)).toBe('image');
		expect(resolveKind('AUDIO', undefined)).toBe('line');
	});
	it('lets a stored kind stand on a dtype that pins none', () => {
		expect(resolveKind('TEXTURE', 'line')).toBe('line');
	});
});

describe('pinnedKind', () => {
	it('names the kind a dtype forces, and nothing for one a viewer may choose', () => {
		expect(pinnedKind('STRING')).toBe('string');
		expect(pinnedKind('TABLE')).toBe('table');
		// The gate on the kind dropdown: an array off a tap is still an array to draw.
		expect(pinnedKind('ARRAY')).toBeNull();
		expect(pinnedKind('TEXTURE')).toBeNull();
		expect(pinnedKind('AUDIO')).toBeNull();
	});
});

describe('isRenderable', () => {
	const spec = (...shape: number[]) => ({ dtype: '<f4', shape, values: new Float32Array(shape.reduce((a, b) => a * b, 1)) });

	it('line draws 1-D and 2-D (C,N) — and nothing higher', () => {
		expect(isRenderable('line', spec(128), {})).toBe(true);
		expect(isRenderable('line', spec(4, 128), {})).toBe(true);
		// A line plot draws nothing above 2-D: a 3-D frame on the default kind must take the same
		// HighDimFallback a 4-D frame gets, not sit blank or frozen on the last 2-D frame.
		expect(isRenderable('line', spec(4, 4, 3), {})).toBe(false);
		expect(isRenderable('line', spec(2, 4, 4, 3), {})).toBe(false);
	});

	it('a trajectory wants pairs of rows; a brain a value per channel or a channel-by-channel matrix', () => {
		const trajectory = { mode: 'trajectory' };
		expect(isRenderable('line', spec(2, 128), trajectory)).toBe(true);
		expect(isRenderable('line', spec(128), trajectory)).toBe(false);
		expect(isRenderable('brain', spec(8), {})).toBe(true);
		expect(isRenderable('brain', spec(8, 8), {})).toBe(true);
		expect(isRenderable('brain', spec(8, 4), {})).toBe(false);
		expect(isRenderable('brain', spec(8), { mode: 'ring' })).toBe(false);
		expect(isRenderable('brain', spec(8, 8), { mode: 'topomap' })).toBe(false);
	});

	it('an image takes a plane or one to four channels', () => {
		expect(isRenderable('image', spec(4, 4), {})).toBe(true);
		expect(isRenderable('image', spec(4, 4, 3), {})).toBe(true);
		expect(isRenderable('image', spec(4, 4, 5), {})).toBe(false);
	});

	it('a non-array frame is always renderable by its own dedicated viewer', () => {
		expect(isRenderable('string', null, {})).toBe(true);
	});
});

describe('resolveSettings', () => {
	it('merges overrides over the kind defaults', () => {
		const merged = resolveSettings('line', { logY: true });
		expect(merged.logY).toBe(true);
		// a default key from the line schema is still present alongside the override
		expect(Object.keys(merged).length).toBeGreaterThan(1);
	});
	it('returns pure defaults when overrides are absent', () => {
		expect(resolveSettings('image', undefined)).toEqual(resolveSettings('image', {}));
	});
	it('image defaults to keeping aspect ratio; stretch is an opt-in override', () => {
		expect(resolveSettings('image', {}).stretch).toBe(false);
		expect(resolveSettings('image', { stretch: true }).stretch).toBe(true);
	});
});
