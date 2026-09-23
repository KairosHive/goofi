import { describe, expect, it } from 'vitest';
import { LinePlot } from './line';

describe('LinePlot', () => {
	it('clears to no range until the next push', () => {
		const p = new LinePlot(() => {}, () => {});
		p.setRect(0, 0, 100, 50);
		p.push({ rows: [Float32Array.of(1, 2, 3)] });
		expect(p.range()).not.toBeNull();
		p.clear();
		expect(p.range()).toBeNull();
		p.push({ rows: [Float32Array.of(4)] });
		expect(p.range()?.scalar).toBe(true);
	});
});
