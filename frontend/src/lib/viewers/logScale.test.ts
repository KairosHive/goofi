import { describe, it, expect } from 'vitest';
import { logSafe } from './logScale';

describe('logSafe', () => {
	it('floors a zero minimum, which is what a PSD bin reaches', () => {
		const [lo, hi] = logSafe(0, 16690.5);
		expect(lo).toBeGreaterThan(0);
		expect(hi).toBe(16690.5);
	});

	it('keeps a range that is already positive', () => {
		expect(logSafe(0.5, 100)).toEqual([0.5, 100]);
	});

	it('floors a negative minimum, as a manual y-range may carry', () => {
		expect(logSafe(-200, 200)[0]).toBeGreaterThan(0);
	});

	it('answers a positive window for every degenerate input', () => {
		for (const [min, max] of [
			[0, 0],
			[-5, -1],
			[NaN, NaN],
			[-Infinity, Infinity],
			[5, 5],
			[100, 1]
		] as [number, number][]) {
			const [lo, hi] = logSafe(min, max);
			expect(lo, `lo for ${min}..${max}`).toBeGreaterThan(0);
			expect(hi, `hi for ${min}..${max}`).toBeGreaterThan(lo);
		}
	});
});
