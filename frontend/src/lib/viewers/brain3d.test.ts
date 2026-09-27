import { describe, expect, it } from 'vitest';
import { project } from './brain3d';
import { lift } from './eegLayout';

describe('the 3-D brain', () => {
	it('looks down at the vertex at pitch 0, with the nose up the screen and the right ear to the right', () => {
		const view = project({ yaw: 0, pitch: 0 }, 200, 100);
		const vertex = view([0, 0, 1]);
		expect([vertex.x, vertex.y, vertex.depth]).toEqual([100, 50, 1]);
		expect(view([0, 1, 0]).y).toBeLessThan(50);
		expect(view([1, 0, 0]).x).toBeGreaterThan(100);
		expect(view([0, 0, -1]).depth).toBe(0);
	});

	it('lifts the layout back onto the sphere: Cz at the vertex, Fpz on the rim in front', () => {
		const [cx, cy, cz] = lift([0.5, 0.5]);
		expect([cx, cy]).toEqual([0, 0]);
		expect(cz).toBeCloseTo(1);
		const [fx, fy, fz] = lift([0.5, 0.05]);
		expect(fx).toBeCloseTo(0);
		expect(fy).toBeCloseTo(1);
		expect(fz).toBeCloseTo(0);
	});
});
