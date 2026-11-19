import { describe, expect, it } from 'vitest';
import type { Track } from '$lib/api/generated';
import type { Curve, Key } from '$lib/api/generated';
import { curveAt } from '$lib/ui/curve';
import { barPlace, barPoint, beatSeconds, contentEnd, gridPath, hiding, keyboard, lanePath, shapeOf, takePath, ticks, turned, zoomed } from './model';

const notes: Track = { name: 'keys', input: '', kind: 'note', notes: [{ at: 2, length: 2, pitch: 90, velocity: 1 }] };
const curve: Track = { name: 'fade', input: '', kind: 'curve', range: [0, 2], rate: 30, keys: [{ at: 1, value: 0, curve: 'linear' }, { at: 3, value: 2, curve: 'step' }] };

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

	it('gives a smooth keyframe the slope of the keyframes beside it', () => {
		const keys = (values: number[], curve: Curve = 'smooth', ats = values.map((_, i) => 2 * i)): Key[] => values.map((value, i) => ({ at: ats[i], value, curve }));
		const blend = (all: Key[], i: number, x: number): number => curveAt(shapeOf(all, i), x);
		// Down a straight fall the value stays on the line, where an easing is level at each keyframe.
		const fall = keys([10, 5, 0]);
		for (const x of [0.05, 0.25, 0.5, 0.95]) expect([blend(fall, 0, x), blend(fall, 1, x)].map((v) => Number(v.toFixed(9)))).toEqual([x, x]);
		expect(blend(keys([10, 5, 0], 'easing'), 0, 0.95)).toBeCloseTo(0.995, 9);
		// At a peak it is level: it comes up to the peak and never past it.
		const peak = keys([0, 10, 0]);
		expect(blend(peak, 0, 0.5)).toBeCloseTo(0.625, 9);
		expect(blend(peak, 0, 0.95)).toBeCloseTo(0.995125, 9);
		expect(blend(peak, 1, 0.05)).toBeCloseTo(1 - 0.995125, 9);
		// A short step after a long fall does not swing the value past its keyframes.
		const kinked = keys([10, 1, 0], 'smooth', [0, 1, 4]);
		for (let x = 0; x <= 1; x += 0.05) for (const i of [0, 1]) expect(blend(kinked, i, x)).toBeGreaterThanOrEqual(0), expect(blend(kinked, i, x)).toBeLessThanOrEqual(1);
		// A keyframe that is not smooth keeps its own curve; a level leg and the last keyframe are a line.
		expect([shapeOf(keys([0, 1, 2], 'step'), 0), shapeOf(keys([3, 3, 5]), 0), shapeOf(fall, 2)]).toEqual(['step', 'linear', 'linear']);
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
		// Past an end the place goes on in proportion, a twelfth of the bar for a length of the
		// axis: twice as far past is twice as far on, to a tenth, for the first length of the axis.
		const past = (lengths: number): number => barPlace(8 + 8 * lengths, 8) - 1;
		expect(past(0.1)).toBeCloseTo(0.4 * 0.1 / 5.1, 12);
		expect(past(1) / past(0.5)).toBeGreaterThan(1.8);
		// Then less and less: half of the most it folds into at five lengths, and never the most.
		expect(past(5)).toBeCloseTo(0.2, 12);
		const far = [1, 10, 100, 1e4, 1e6].map(past);
		expect(far.every((p, i) => p < 0.4 && (i === 0 || p > far[i - 1]))).toBe(true);
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

	it('zooms a view about a point that stays where it is', () => {
		// A turn up looks closer and a turn down further, each the other's way back.
		const near = zoomed([0, 8], 2, -100);
		expect(near[1] - near[0]).toBeLessThan(8);
		expect((2 - near[0]) / (near[1] - near[0])).toBeCloseTo(0.25, 9);
		expect(zoomed(near, 2, 100).map((v) => Number(v.toFixed(9)))).toEqual([0, 8]);
		// About an end of the view, that end stays.
		expect(zoomed([4, 8], 4, 200)[0]).toBe(4);
		// A view gets no narrower than 0.05 of a beat, and no wider than a million.
		const [closest, furthest] = [zoomed([0, 8], 4, -1e5), zoomed([0, 8], 4, 1e5)];
		expect(closest[1] - closest[0]).toBeCloseTo(0.05, 9);
		expect(furthest[1] - furthest[0]).toBeCloseTo(1e6, 3);
	});

	it('draws a take as a line through its samples, begun again at each pass', () => {
		const along = (at: number): number => at / 4;
		expect(takePath([], [0, 1], along)).toBe('');
		expect(takePath([0, 0, 2, 1, 1, 0.5, 3, 0.5], [0, 1], along)).toBe('M0 100 L500 0 M250 50 L750 50');
		expect(takePath([0, 0, 2, 1], [0, 1], along, true)).toBe('M0 0 L100 500');
	});
});

describe('keyboard', () => {
	it('shows two octaves from C3 and every note the track holds, but for what is set', () => {
		expect(keyboard([])).toEqual({ range: [48, 72], scale: 'chromatic', root: 'C', grid: '1/4 beat' });
		expect(keyboard(notes.kind === 'note' ? notes.notes : []).range).toEqual([48, 90]);
		expect(keyboard([{ at: 0, length: 1, pitch: 30, velocity: 1 }], { range: [60, 72], grid: '1 s' })).toEqual({ range: [60, 72], scale: 'chromatic', root: 'C', grid: '1 s' });
		// What no keyboard has, from a file or an agent, is as if unset.
		expect(keyboard([], { range: [72, 60], scale: 'none', root: 'H', grid: '1/7 beat' })).toEqual(keyboard([]));
		expect(keyboard([], { range: [60.5, 200] as [number, number] }).range).toEqual([48, 72]);
	});
});
