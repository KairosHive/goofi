import { describe, expect, it } from 'vitest';
import { curveAt, curvePath, handlesOf, PRESETS } from './curve';

describe('curveAt', () => {
	it('runs every preset from 0 to 1 and never backwards', () => {
		for (const p of PRESETS) {
			expect(curveAt(p, 0)).toBe(0);
			expect(curveAt(p, 1)).toBe(1);
			let last = 0;
			for (let i = 1; i <= 20; i++) {
				const y = curveAt(p, i / 20);
				expect(y).toBeGreaterThanOrEqual(last);
				last = y;
			}
		}
	});
	it('solves a bezier for its x, and a symmetric one crosses the middle', () => {
		const ease = { bezier: [0.42, 0, 0.58, 1] as [number, number, number, number] };
		expect(curveAt(ease, 0)).toBe(0);
		expect(curveAt(ease, 1)).toBe(1);
		expect(curveAt(ease, 0.5)).toBeCloseTo(0.5, 6);
		expect(curveAt(ease, 0.25)).toBeLessThan(0.25);
		expect(curveAt({ bezier: [0.1, 0.9, 0.3, 1] }, 0.25)).toBeGreaterThan(0.25);
	});
	it('seeds a custom curve with the shape of the preset it leaves', () => {
		// A step has no bezier: its seed is the steepest corner, checked at its ends alone.
		for (const p of PRESETS) {
			const h = handlesOf(p);
			for (let i = 0; i <= 10; i++) {
				const x = i / 10;
				if (p === 'step' && i % 10 !== 0) continue;
				expect(Math.abs(curveAt({ bezier: h }, x) - curveAt(p, x))).toBeLessThan(0.1);
			}
		}
	});
	it('draws a path that starts at the bottom left and ends at the top right', () => {
		expect(curvePath('linear', 10, 10, 2)).toBe('M0.00 10.00 L5.00 5.00 L10.00 0.00');
		expect(curvePath('step', 10, 10)).toBe('M0 10 L10 10 L10 0');
	});
});
