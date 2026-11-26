import { describe, expect, it } from 'vitest';
import { bisect, oklchHex, seriesColor } from './color';

describe('series palette', () => {
	it('bisects the widest arc without moving earlier points', () => {
		expect([0, 1, 2, 3, 4, 5, 6, 7].map(bisect)).toEqual([0, 0.5, 0.25, 0.75, 0.125, 0.625, 0.375, 0.875]);
		for (const n of [8, 16, 64]) {
			const pts = Array.from({ length: n }, (_, i) => bisect(i)).sort((a, b) => a - b);
			const gaps = pts.map((p, k) => (k + 1 < n ? pts[k + 1] - p : 1 - p));
			expect(Math.min(...gaps)).toBeCloseTo(1 / n);
		}
		const pts = Array.from({ length: 11 }, (_, i) => bisect(i)).sort((a, b) => a - b);
		const gaps = pts.map((p, k) => (k + 1 < 11 ? pts[k + 1] - p : 1 - p));
		expect(Math.min(...gaps)).toBeGreaterThanOrEqual(1 / 22);
	});
	it('starts blue and never repeats within sixty-four series', () => {
		const [r, , b] = [1, 3, 5].map((k) => parseInt(seriesColor(0).slice(k, k + 2), 16));
		expect(b - r).toBeGreaterThan(40);
		const seen = new Set(Array.from({ length: 64 }, (_, i) => seriesColor(i)));
		expect(seen.size).toBe(64);
	});
	it('keeps every colour inside the sRGB gamut', () => {
		for (let i = 0; i < 64; i++) expect(seriesColor(i)).toMatch(/^#[0-9a-f]{6}$/);
		expect(oklchHex(0.78, 0, 0)).toMatch(/^#([0-9a-f]{2})\1\1$/);
	});
});
