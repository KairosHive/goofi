import { describe, expect, it } from 'vitest';
import type { Track } from '$lib/api/generated';
import { contentEnd, gridPath, hiding, lanePath, ticks, unit } from './model';

const notes: Track = { name: 'keys', kind: 'note', range: [48, 72], scale: 'chromatic', root: 'C', notes: [{ at: 2, length: 2, pitch: 90, velocity: 1 }] };
const curve: Track = { name: 'fade', kind: 'curve', range: [0, 2], keys: [{ at: 1, value: 0, curve: 'linear' }, { at: 3, value: 2, curve: 'step' }] };

describe('the timeline model', () => {
	it('reads a beat by the tempo, and finds where the tracks end', () => {
		expect(unit(true, 120)).toBe(0.5);
		expect(unit(false, 120)).toBe(1);
		expect(contentEnd([notes, curve])).toBe(4);
		expect(contentEnd([])).toBe(0);
	});

	it('marks the ruler at a round step, over what is in view', () => {
		expect(ticks(0, 8)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8]);
		expect(ticks(0, 60)).toEqual([0, 10, 20, 30, 40, 50, 60]);
		expect(ticks(0, 1.5)).toEqual([0, 0.2, 0.4, 0.6, 0.8, 1, 1.2, 1.4]);
		expect(ticks(-3, 5)).toEqual([-3, -2, -1, 0, 1, 2, 3, 4, 5]);
		expect(ticks(100.5, 103)).toEqual([100.5, 101, 101.5, 102, 102.5, 103]);
		expect(ticks(1000000, 1000008)).toEqual([1000000, 1000001, 1000002, 1000003, 1000004, 1000005, 1000006, 1000007, 1000008]);
	});

	it('draws a curve held before its first keyframe and after its last', () => {
		if (curve.kind !== 'curve') throw new Error('a curve track');
		const along = (at: number): number => at / 4;
		expect(lanePath(curve.keys, curve.range, along, false, 2)).toBe('M0 100 L250 100 L500 50 L750 0 L1000 0');
		// Flipped, the time runs down and the value to the right.
		expect(lanePath(curve.keys, curve.range, along, true, 2)).toBe('M0 0 L0 250 L50 500 L100 750 L100 1000');
		// A view that begins after the first keyframe starts the line at that keyframe, out of view.
		expect(lanePath(curve.keys, curve.range, (at) => (at - 2) / 2, false, 1)).toBe('M-500 100 L-500 100 L500 0 L1000 0');
		expect(lanePath([], [0, 1], along)).toBe('');
	});

	it('draws a line between the cells in view, and fewer when they are too many', () => {
		expect(gridPath(0, 1, 0.25)).toBe('M0 0V100M250 0V100M500 0V100M750 0V100M1000 0V100');
		expect(gridPath(0.1, 0.6, 0.25, true)).toBe('M0 300H100M0 800H100');
		expect(gridPath(0, 1000, 0.25).split('M').length - 1).toBe(126);
	});

	it('names the tracks each menu entry hides', () => {
		const shown = ['k-1', 'k-2', 'k-3'];
		expect(hiding(shown, 'k-2', 'this')).toEqual(['k-2']);
		expect(hiding(shown, 'k-2', 'above')).toEqual(['k-1']);
		expect(hiding(shown, 'k-2', 'below')).toEqual(['k-3']);
		expect(hiding(shown, 'k-2', 'others')).toEqual(['k-1', 'k-3']);
		expect(hiding(shown, 'k-1', 'above')).toEqual([]);
	});
});
