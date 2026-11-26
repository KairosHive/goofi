import { describe, expect, it } from 'vitest';
import { paths, PATH_STRIDE } from './path';

describe('paths', () => {
	it('lays strokes out one pad apart, dots one after another, both padded for the instanced read', () => {
		const d = paths()
			.stroke([0, 0, 1, 1], 2, '#ff0000')
			.stroke([0.5, 0.5, 0.5, 1], 1, [0, 1, 0, 0.5])
			.dot(0.25, 0.75, 6, '#0000ff')
			.build();
		const nan = (v: number) => (Number.isNaN(v) ? 'nan' : v);
		const vertex = (buf: Float32Array, k: number) => [...buf.subarray(k * PATH_STRIDE, (k + 1) * PATH_STRIDE)].map(nan);
		expect(vertex(d.strokes, 0)).toEqual(Array(7).fill('nan'));
		expect(vertex(d.strokes, 1)).toEqual([0, 0, 2, 1, 0, 0, 1]);
		expect(vertex(d.strokes, 2)).toEqual([1, 1, 2, 1, 0, 0, 1]);
		expect(vertex(d.strokes, 3)).toEqual(Array(7).fill('nan'));
		expect(vertex(d.strokes, 5)[3]).toBe(0);
		expect(vertex(d.strokes, 5)[6]).toBe(0.5);
		// Two strokes of two points: a pad, four vertices, two pads between and after, one more after.
		expect(d.strokes.length / PATH_STRIDE - 3).toBe(5);
		expect(vertex(d.dots, 1)).toEqual([0.25, 0.75, 6, 0, 0, 1, 1]);
		expect(d.dots.length / PATH_STRIDE - 3).toBe(1);
	});
});
