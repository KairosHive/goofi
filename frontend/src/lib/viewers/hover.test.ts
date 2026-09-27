import { describe, expect, it } from 'vitest';
import { imageProbe, lineProbe } from './hover';
import type { ArrayData } from '$lib/codec/decode';

const axes = { logX: false, logY: false, pad: 0 };
const box = { w: 100, h: 100, tol: 10 };

describe('lineProbe', () => {
	// Two series over 11 samples; the plot range is the drawn window.
	const rows = [Float32Array.from({ length: 11 }, (_, i) => i / 10), new Float32Array(11).fill(0.5)];
	const range = { xMin: 0, xMax: 10, yMin: 0, yMax: 1, scalar: false };

	it('reads the nearest vertex and names it by the axes', () => {
		const probe = lineProbe({ rows, base: 0 }, range, axes, {
			x: Array.from({ length: 11 }, (_, i) => `t${i}`),
			series: ['ramp', 'flat']
		});
		const h = probe(31, 71, box)!;
		expect(h.lines).toEqual(['ramp', 'x t3', 'y 0.300']);
		expect(h.mark!.x).toBeCloseTo(30);
		expect(h.mark!.y).toBeCloseTo(70);
		expect(probe(31, 52, box)!.lines).toEqual(['flat', 'x t3', 'y 0.500']);
	});

	it('answers nothing away from every line, and nothing for a scalar bar', () => {
		const probe = lineProbe({ rows, base: 0 }, range, axes, { x: null, series: null });
		expect(probe(30, 20, box)).toBeNull();
		expect(lineProbe({ rows: [[1]] }, { ...range, scalar: true }, axes, { x: null, series: null })(5, 5, box)).toBeNull();
	});

	it('follows decimated positions and an unnamed index', () => {
		const probe = lineProbe({ rows: [[0.2, 0.8]], xs: [0, 10] }, range, axes, { x: null, series: null });
		expect(probe(98, 22, box)!.lines).toEqual(['x 10', 'y 0.800']);
	});
});

describe('imageProbe', () => {
	const arr: ArrayData = {
		dtype: 'float32',
		shape: [2, 4],
		values: Float32Array.from([1, 2, 3, 4, 5, 6, 7, 8])
	};

	it('reads the cell under the pointer in a fitted image', () => {
		const probe = imageProbe(arr, false, { channels: { dim0: ['top', 'bottom'] } });
		// A 4×2 image in a 100×100 box sits at y 25..75, 25 px a cell.
		expect(probe(60, 60, box)!.lines).toEqual(['x 2', 'y bottom', '7.00']);
		expect(probe(60, 10, box)).toBeNull();
	});

	it('spans the box when stretched', () => {
		const probe = imageProbe(arr, true, {});
		expect(probe(99, 1, box)!.lines).toEqual(['x 3', 'y 0', '4.00']);
	});
});
