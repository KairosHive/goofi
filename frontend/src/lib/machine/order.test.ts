import { describe, expect, it } from 'vitest';
import type { Machine } from '$lib/api/generated';
import { columnOrder, joined, widths } from './order';

/** A machine of `states`, wired by `edges` written `A>B`. */
function machine(states: string[], edges: string[], start?: string): Machine {
	const m: Machine = { attributes: {}, states: {}, transitions: {}, playheads: {} };
	for (const s of states) m.states[s] = { pos: [0, 0], interval: 1, mode: 'random', noop: 0, weights: {}, expression: '' };
	edges.forEach((e, i) => {
		const [from, to] = e.split('>');
		m.transitions[`t${i + 1}`] = { from, to, duration: 0 };
	});
	if (start) m.playheads.p = { color: '', start };
	return m;
}

describe('columnOrder', () => {
	it('leads with the largest loop, from the playhead start', () => {
		const m = machine(['A', 'B', 'C', 'D'], ['A>B', 'B>C', 'C>A', 'C>D', 'D>C'], 'B');
		expect(columnOrder(m)).toEqual(['B', 'C', 'D', 'A']);
	});
	it('opens a smaller loop at the state it shares', () => {
		const m = machine(['A', 'B', 'C', 'D', 'E'], ['A>B', 'B>C', 'C>D', 'D>A', 'B>E', 'E>B'], 'A');
		expect(columnOrder(m)).toEqual(['A', 'B', 'E', 'C', 'D']);
	});
	it('hangs a chain after the state that leads to it, and a loop reached by a door after its door', () => {
		const m = machine(['A', 'B', 'X', 'Y', 'P', 'Q'], ['A>B', 'B>A', 'B>X', 'X>Y', 'A>P', 'P>Q', 'Q>P'], 'A');
		expect(columnOrder(m)).toEqual(['A', 'P', 'Q', 'B', 'X', 'Y']);
	});
	it('keeps the document order where nothing is wired, and skips a self loop', () => {
		expect(columnOrder(machine(['C', 'A', 'B'], ['A>A']))).toEqual(['C', 'A', 'B']);
		expect(columnOrder(machine([], []))).toEqual([]);
	});
	it('joins neighbours either way, and through a * transition', () => {
		const m = machine(['A', 'B', 'C'], ['A>B', '*>C']);
		expect(joined(m, 'A', 'B')).toBe(true);
		expect(joined(m, 'B', 'A')).toBe(true);
		expect(joined(m, 'B', 'C')).toBe(true);
		expect(joined(m, 'C', 'A')).toBe(true);
		expect(joined(machine(['A', 'B'], []), 'A', 'B')).toBe(false);
	});
});

describe('widths', () => {
	it('shares the width by duration, floors the short, and splits evenly where nothing lasts', () => {
		expect(widths([1, 3], 400, 50)).toEqual([100, 300]);
		expect(widths([0.1, 10], 400, 50)).toEqual([50, 350]);
		expect(widths([0, 0, 0], 300, 50)).toEqual([100, 100, 100]);
		expect(widths([1, 1, 1], 60, 50)).toEqual([50, 50, 50]);
		expect(widths([], 100, 50)).toEqual([]);
	});
});
