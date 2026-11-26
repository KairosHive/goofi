import { describe, expect, it } from 'vitest';
import { LinePlot } from './line';

describe('LinePlot', () => {
	it('keeps its autoscaled range under noise on the extent, and until a clear or a setting', () => {
		const p = new LinePlot(() => {}, () => {});
		p.push({ rows: [Float32Array.of(-1, 0, 1)] });
		const first = p.range();
		p.push({ rows: [Float32Array.of(-0.9, 0, 0.95)] });
		expect(p.range(), 'a slightly smaller extent moves no bound').toEqual(first);
		p.push({ rows: [Float32Array.of(-0.9, 0, 2)] });
		expect(p.range()?.yMax).toBeCloseTo(2 + 2.9 * 0.05);
		p.push({ rows: [Float32Array.of(-0.1, 0, 0.1)] });
		expect(p.range()?.yMax, 'a signal that fell far inside gets its own window').toBeCloseTo(0.11);
		// A setting starts the fit afresh: the held window belonged to the old axis.
		p.push({ rows: [Float32Array.of(-0.09, 0, 0.09)] });
		expect(p.range()?.yMax).toBeCloseTo(0.11);
		p.setSettings({ logY: false });
		expect(p.range()?.yMax).toBeCloseTo(0.099);
	});
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
