import { describe, expect, it } from 'vitest';
import type { Track } from '$lib/api/generated';
import { contentEnd, hiding, lanePath, pitches, ticks, unit } from './model';

const notes: Track = { name: 'keys', beats: true, kind: 'note', notes: [{ at: 2, length: 2, pitch: 90, velocity: 1 }] };
const curve: Track = {
	name: 'fade',
	beats: false,
	kind: 'curve',
	attributes: { 'a-1': { name: 'level', range: [0, 2], keys: [{ at: 1, value: 0, curve: 'linear' }, { at: 3, value: 2, curve: 'step' }] } }
};

describe('the timeline model', () => {
	it('reads a time in beats by the tempo, and in seconds as it is', () => {
		expect(unit(true, 120)).toBe(0.5);
		expect(unit(false, 120)).toBe(1);
		expect(contentEnd([notes, curve], 120)).toBe(3);
		expect(contentEnd([notes, curve], 30)).toBe(8);
		expect(contentEnd([], 120)).toBe(0);
	});

	it('marks the ruler at a round step', () => {
		expect(ticks(8)).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8]);
		expect(ticks(60)).toEqual([0, 10, 20, 30, 40, 50, 60]);
		expect(ticks(1.5)).toEqual([0, 0.2, 0.4, 0.6, 0.8, 1, 1.2, 1.4]);
	});

	it('widens a note lane to hold every note', () => {
		expect(pitches([])).toEqual([48, 72]);
		expect(pitches(notes.notes)).toEqual([48, 92]);
	});

	it('draws a lane held before its first keyframe and after its last', () => {
		const path = lanePath(curve.attributes['a-1'], 4, 400, 100, 2);
		expect(path).toBe('M0 100 L100 100 L200 50 L300 0 L400 0');
		expect(lanePath({ name: 'none', range: [0, 1], keys: [] }, 4, 400, 100)).toBe('');
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
