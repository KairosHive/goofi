import { describe, expect, it } from 'vitest';
import { curveAt, curvePath, fitted, pointsOf, PRESETS, type Point } from './curve';

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
	it('travels a custom curve segment by segment, through every anchor point', () => {
		const ease = { custom: pointsOf('easing') };
		expect(curveAt(ease, 0)).toBeCloseTo(0, 6);
		expect(curveAt(ease, 1)).toBeCloseTo(1, 6);
		expect(curveAt(ease, 0.5)).toBeCloseTo(0.5, 6);
		expect(curveAt(ease, 0.25)).toBeLessThan(0.25);
		// Three points: the curve is at the middle one where that one stands.
		const bent: Point[] = [[0, 0, 0, 0, 0.1, 0], [0.5, 0.8, -0.1, 0, 0.1, 0], [1, 1, -0.1, 0, 0, 0]];
		expect(curveAt({ custom: bent }, 0.5)).toBeCloseTo(0.8, 6);
		expect(curveAt({ custom: bent }, 0.25)).toBeGreaterThan(0.25);
	});
	it('seeds an edited curve with the shape of the preset it leaves', () => {
		// A step is no curve: its seed is the steepest corner, checked at its ends alone.
		for (const p of PRESETS) {
			const custom = pointsOf(p);
			for (let i = 0; i <= 10; i++) {
				const x = i / 10;
				if (p === 'step' && i % 10 !== 0) continue;
				expect(Math.abs(curveAt({ custom }, x) - curveAt(p, x))).toBeLessThan(0.1);
			}
		}
	});
	it('reads one bezier with handles a third and two thirds along as a cubic of the progress', () => {
		// What a smooth keyframe is: the same blend the bisection finds, with no search.
		const cubic: Point[] = [[0, 0, 0, 0, 1 / 3, 0.2], [1, 1, -1 / 3, -0.5, 0, 0]];
		const near: Point[] = [[0, 0, 0, 0, 1 / 3 + 1e-9, 0.2], [1, 1, -1 / 3, -0.5, 0, 0]];
		for (const x of [0, 0.1, 0.5, 0.9, 1]) expect(curveAt({ custom: cubic }, x)).toBeCloseTo(curveAt({ custom: near }, x), 6);
		expect(curveAt('smooth', 0.3), 'alone, smooth is a straight line').toBe(0.3);
	});
	it('keeps a handle inside the segment it shapes', () => {
		const loose: Point[] = [[0, 0, -1, 1, 0.9, 0], [0.5, 0.5, -0.9, 0, 0.9, 0], [1, 1, -0.9, 0, 1, 1]];
		expect(fitted(loose)).toEqual([[0, 0, 0, 0, 0.5, 0], [0.5, 0.5, -0.5, 0, 0.5, 0], [1, 1, -0.5, 0, 0, 0]]);
	});
	it('draws a path that starts at the bottom left and ends at the top right', () => {
		expect(curvePath('linear', 10, 10, 2)).toBe('M0 10 L5 5 L10 0');
		expect(curvePath('step', 10, 10)).toBe('M0 10 L10 10 L10 0');
		expect(curvePath({ custom: pointsOf('easing') }, 10, 10)).toBe('M0 10 C4.2 10 5.8 0 10 0');
	});
});
