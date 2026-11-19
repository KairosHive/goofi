import { describe, expect, it } from 'vitest';
import type { Track } from '$lib/api/generated';
import { barPlace, barPoint, beatSeconds, contentEnd, gridPath, hiding, lanePath, takePath, ticks, turned } from './model';

const notes: Track = { name: 'keys', order: 0, input: '', kind: 'note', range: [48, 72], scale: 'chromatic', root: 'C', grid: '1/4 beat', notes: [{ at: 2, length: 2, pitch: 90, velocity: 1 }] };
const curve: Track = { name: 'fade', order: 1, input: '', kind: 'curve', range: [0, 2], rate: 30, keys: [{ at: 1, value: 0, curve: 'linear' }, { at: 3, value: 2, curve: 'step' }] };

describe('the timeline model', () => {
	it('reads a beat by the tempo, and finds where the tracks end', () => {
		expect(beatSeconds(120)).toBe(0.5);
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

	it('folds a view past an end of the axis into the end of its bar', () => {
		// Inside the axis a point stands where it is: a view of the whole axis is the whole bar.
		expect([0, 4, 8].map((at) => barPlace(at, 8))).toEqual([0, 0.5, 1]);
		// Past an end it is that far past at first, then less and less, and never 0.15 of the bar.
		expect(barPlace(8.08, 8) - 1).toBeCloseTo(0.01, 2);
		const far = [9, 16, 80, 8000].map((at) => barPlace(at, 8) - 1);
		expect(far.every((p, i) => p < 0.15 && (i === 0 || p > far[i - 1]))).toBe(true);
		expect(barPlace(-16, 8)).toBeCloseTo(1 - barPlace(24, 8), 12);
		// A place on the bar is one point of the axis, there and back.
		for (const at of [-40, -1, 0, 3, 8, 9, 100]) expect(barPoint(barPlace(at, 8), 8)).toBeCloseTo(at, 6);
		// Past where a fold ends, the place is the furthest point a fold holds.
		expect(barPoint(2, 8)).toBe(barPoint(1.5, 8));
	});

	it('takes a turn of the wheel along the axis, and leaves a turn across it', () => {
		const turn = (deltaX: number, deltaY: number, shiftKey = false, deltaMode = 0) => ({ deltaX, deltaY, deltaMode, shiftKey });
		// Sideways along an axis that runs to the right, down along one that runs down.
		expect([turned(turn(40, 0), false, 500), turned(turn(0, 40), false, 500)]).toEqual([40, 0]);
		expect([turned(turn(40, 0), true, 500), turned(turn(0, -40), true, 500)]).toEqual([0, -40]);
		// A slanted swipe goes the way it goes most, and one that goes both ways alike is across.
		expect([turned(turn(40, 10), false, 500), turned(turn(10, 40), false, 500), turned(turn(40, 40), false, 500)]).toEqual([40, 0, 0]);
		// Shift turns a wheel with one direction sideways, said in the turn or by the key alone.
		expect([turned(turn(0, 40, true), false, 500), turned(turn(40, 0, true), false, 500)]).toEqual([40, 40]);
		expect(turned(turn(0, 40, true), true, 500)).toBe(0);
		// A turn counted in lines or in pages is that many pixels.
		expect([turned(turn(3, 0, false, 1), false, 500), turned(turn(1, 0, false, 2), false, 500)]).toEqual([48, 500]);
	});

	it('draws a take as a line through its samples, begun again at each pass', () => {
		const along = (at: number): number => at / 4;
		expect(takePath([], [0, 1], along)).toBe('');
		expect(takePath([0, 0, 2, 1, 1, 0.5, 3, 0.5], [0, 1], along)).toBe('M0 100 L500 0 M250 50 L750 50');
		expect(takePath([0, 0, 2, 1], [0, 1], along, true)).toBe('M0 0 L100 500');
	});
});
