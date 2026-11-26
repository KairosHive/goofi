import { describe, expect, it } from 'vitest';
import {
	axisWindow,
	clipped,
	damage,
	deviceRect,
	extent,
	fitImage,
	gridLines,
	layoutSeries,
	logSafe,
	meets,
	rgba
} from './math';

describe('rects', () => {
	const view = { x: 100, y: 50, zoom: 0.5, width: 800, height: 600, dpr: 2 };
	it('places a flow rect on the device through camera and dpr, in whole pixels', () => {
		expect(deviceRect({ x: 20, y: 40, w: 233, h: 144 }, view)).toEqual({ x: 220, y: 140, w: 233, h: 144 });
	});
	it('rounds the far edge, not the size, so neighbours never overlap or gap', () => {
		const a = deviceRect({ x: 0, y: 0, w: 10.3, h: 10 }, { ...view, zoom: 1, dpr: 1, x: 0, y: 0 });
		const b = deviceRect({ x: 10.3, y: 0, w: 10.3, h: 10 }, { ...view, zoom: 1, dpr: 1, x: 0, y: 0 });
		expect(a.x + a.w).toBe(b.x);
	});
	it('keeps the part of a rect inside the pane, and culls one with none or with no area', () => {
		expect(clipped({ x: -10, y: -10, w: 20, h: 20 }, 1600, 1200)).toEqual({ x: 0, y: 0, w: 10, h: 10 });
		expect(clipped({ x: 1590, y: 1190, w: 20, h: 20 }, 1600, 1200)).toEqual({ x: 1590, y: 1190, w: 10, h: 10 });
		expect(clipped({ x: -30, y: 0, w: 20, h: 20 }, 1600, 1200)).toBeNull();
		expect(clipped({ x: 1600, y: 0, w: 20, h: 20 }, 1600, 1200)).toBeNull();
		expect(clipped({ x: 0, y: 1200, w: 20, h: 20 }, 1600, 1200)).toBeNull();
		expect(clipped({ x: 0, y: 0, w: 0, h: 20 }, 1600, 1200)).toBeNull();
	});
	it('repaints each changed rect once, a neighbour that shares a pixel with it, and nothing else', () => {
		const a = { x: 0, y: 0, w: 100, h: 50 };
		const moved = { x: 40, y: 0, w: 100, h: 50 };
		const inner = { x: 10, y: 10, w: 20, h: 20 };
		// A plot that pushed is one rect; one that moved is where it was and where it is.
		expect(damage([a, a])).toEqual([a]);
		expect(damage([a, moved])).toEqual([a, moved]);
		// A rect another holds whole is drawn with it, whichever came first.
		expect(damage([inner, a])).toEqual([a]);
		expect(damage([a, inner])).toEqual([a]);
		expect(damage([])).toEqual([]);
		expect(meets(a, moved)).toBe(true);
		expect(meets(a, { x: 100, y: 0, w: 100, h: 50 }), 'a shared edge is no shared pixel').toBe(false);
		expect(meets(a, { x: 0, y: 50, w: 100, h: 50 })).toBe(false);
	});
	it('letterboxes an image inside its rect unless stretched', () => {
		const d = { x: 10, y: 10, w: 200, h: 100 };
		expect(fitImage(d, 50, 50, false)).toEqual({ x: 60, y: 10, w: 100, h: 100 });
		expect(fitImage(d, 50, 50, true)).toEqual(d);
	});
});

describe('windows', () => {
	it('floors a log window to positive finite bounds', () => {
		expect(logSafe(0, 100)).toEqual([0.1, 100]);
		expect(logSafe(NaN, NaN)).toEqual([1e-3, 1]);
		expect(logSafe(5, 5)).toEqual([5e-3, 5]);
	});
	it('pads a linear window and opens a flat one', () => {
		expect(axisWindow(0, 10, false)).toEqual([-0.5, 10.5]);
		expect(axisWindow(3, 3, false)).toEqual([2, 4]);
		expect(axisWindow(10, 0, false, 0)).toEqual([0, 10]);
	});
	it('works in log10 space for a log axis', () => {
		const [lo, hi] = axisWindow(1, 1000, true, 0);
		expect(lo).toBeCloseTo(0);
		expect(hi).toBeCloseTo(3);
	});
	it('takes the extent of finite values only, positive ones on request', () => {
		const v = new Float32Array([NaN, -2, 0, 3, Infinity]);
		expect(extent(v)).toEqual([-2, 3]);
		expect(extent(v, 0, 1, true)).toEqual([3, 3]);
		expect(extent(new Float32Array([NaN]))).toBeNull();
		expect(extent(new Float32Array([1, 10, 2, 20]), 1, 2)).toEqual([10, 20]);
		// A float32 denormal is underflow residue, not a value a log axis should reach down to.
		expect(extent(new Float32Array([6.78e-41, 3.5e-43, 2, 30]), 0, 1, true)).toEqual([2, 30]);
		expect(extent(new Float32Array([2e-38, 2]), 0, 1, true)?.[0]).toBeCloseTo(2e-38, 45);
	});
});

describe('series layout', () => {
	it('interleaves x and y with a NaN pad after each row', () => {
		const { buf, stride, m } = layoutSeries([[1, 2], [3, 4]], null, 0, null);
		expect(m).toBe(2);
		expect(stride).toBe(3);
		expect([...buf].map((v) => (Number.isNaN(v) ? 'nan' : v))).toEqual([
			'nan', 'nan', 0, 1, 1, 2, 'nan', 'nan', 0, 3, 1, 4, 'nan', 'nan', 'nan', 'nan'
		]);
	});
	it('takes a row of xs per series', () => {
		const { buf } = layoutSeries([[1, 2], [3, 4]], [[5, 6], [7, 8]], 0, null);
		expect([...buf].slice(2, 6)).toEqual([5, 1, 6, 2]);
		expect([...buf].slice(8, 12)).toEqual([7, 3, 8, 4]);
	});
	it('takes explicit xs and a base, and reuses a buffer that fits', () => {
		const first = layoutSeries([[5]], null, 1, null);
		expect([...first.buf].slice(2, 4)).toEqual([1, 5]);
		const again = layoutSeries([[6]], [7], 0, first.buf);
		expect(again.buf).toBe(first.buf);
		expect([...again.buf].slice(2, 4)).toEqual([7, 6]);
	});
});

describe('colour', () => {
	it('parses three, six and eight digit hex', () => {
		expect(rgba('#ff0000')).toEqual([1, 0, 0, 1]);
		expect(rgba('#111')).toEqual(rgba('#111111'));
		expect(rgba('#00ff0080')[3]).toBeCloseTo(0.502, 3);
	});
});

describe('gridLines', () => {
	it('places 1-2-5 major steps inside a linear window, never on its edges', () => {
		const g = gridLines(0, 10, false);
		expect(g.major.map((v) => Math.round(v * 10))).toEqual([2, 4, 6, 8]);
		expect(g.minor).toEqual([]);
		expect(gridLines(-1.05, 1.05, false).major.length).toBeGreaterThan(2);
		expect(gridLines(3, 3, false).major).toEqual([]);
	});
	it('marks decades as major and mantissa steps as minor on a log window', () => {
		const g = gridLines(-0.1, 3.1, true);
		expect(g.major.length).toBe(4);
		expect(g.major[0]).toBeCloseTo(0.1 / 3.2);
		expect(g.minor.length).toBe(3 * 8 + 1); // three full decades, plus the 9 of the one below
		expect(g.minor.some((v) => Math.abs(v - (0.1 + Math.log10(2)) / 3.2) < 1e-9)).toBe(true);
		expect(g.minor.every((v) => v > 0 && v < 1)).toBe(true);
		expect(gridLines(0, 12, true).minor).toEqual([]);
	});
});
