import { describe, expect, it } from 'vitest';
import type { ParamDescriptor } from '$lib/api/types';
import {
	admits,
	filteredRows,
	isModified,
	narrowing,
	onlyAdmitted,
	settleNonDefault,
	SHOW_ALL,
	toggleSource,
	type Baseline,
	type Filters,
	type NonDefault
} from './paramFilters';

const base = {
	doc: null,
	refreshable: false,
	expression: null,
	mode: 'constant',
	reference: null,
	triggers: false,
	error: null
} as const;

const float = (value: number, dflt: number): ParamDescriptor =>
	({ ...base, type: 'float', value, default: dflt, vmin: 0, vmax: 1 }) as ParamDescriptor;

const expr = (text: string, value = 0.5, dflt = 0.5): ParamDescriptor =>
	({ ...float(value, dflt), mode: 'expression', expression: text }) as ParamDescriptor;

const ref = (target: string, value = 0.5, dflt = 0.5): ParamDescriptor =>
	({ ...float(value, dflt), mode: 'reference', reference: target }) as ParamDescriptor;

const F = (f: Partial<Filters>): Filters => ({ ...SHOW_ALL, ...f });

/** The list a settled document leaves, from nothing — what the inspector's effect holds. */
const listOf = (groups: Parameters<typeof settleNonDefault>[1], b?: Baseline): NonDefault =>
	settleNonDefault(new Map(), groups, b);
const NONE: NonDefault = new Map();

describe('isModified', () => {
	it('is false for a param still sitting on its declared default', () => {
		expect(isModified(float(0.5, 0.5))).toBe(false);
	});

	it('is true once the value has moved — which is how a knob turned in a plugin window shows up', () => {
		expect(isModified(float(0.7, 0.5))).toBe(true);
	});

	it('is true for a newly bound expression, because binding one IS a change', () => {
		expect(isModified(expr('t'))).toBe(true);
	});

	it('is false for a pulse, which holds no value to compare', () => {
		const pulse = { ...base, type: 'pulse', value: null, default: null } as ParamDescriptor;
		expect(isModified(pulse)).toBe(false);
	});

	it('treats a string param off its default as non-default', () => {
		const s = { ...base, type: 'string', value: 'hard', default: 'soft', options: null } as ParamDescriptor;
		expect(isModified(s)).toBe(true);
	});

	// The whole point of Clear: a preset moved these, and after clearing they are the new zero.
	it('is false once a baseline records the value it now holds', () => {
		const b: Baseline = { 'common/frequency': { value: 0.7, mode: 'constant', expression: '', reference: '' } };
		expect(isModified(float(0.7, 0.5), b, 'common', 'frequency')).toBe(false);
	});

	it('is true again once the value moves off the recorded baseline', () => {
		const b: Baseline = { 'common/frequency': { value: 0.7, mode: 'constant', expression: '', reference: '' } };
		expect(isModified(float(0.9, 0.5), b, 'common', 'frequency')).toBe(true);
	});

	// A mapping survives Clear as a MAPPING — it keeps driving, and keeps answering the source
	// strip — but it stops counting as changed, which is what makes the list quiet again.
	it('is false for an expression the baseline already recorded', () => {
		const b: Baseline = { 'common/frequency': { value: 0.5, mode: 'expression', expression: 't', reference: '' } };
		expect(isModified(expr('t'), b, 'common', 'frequency')).toBe(false);
	});

	it('is true when the expression text itself changes after a clear', () => {
		const b: Baseline = { 'common/frequency': { value: 0.5, mode: 'expression', expression: 't', reference: '' } };
		expect(isModified(expr('t * 2'), b, 'common', 'frequency')).toBe(true);
	});

	// The other two texts are RETAINED across a mode switch, so only the ACTIVE one is compared.
	it('ignores an expression left behind by a param now back on a constant', () => {
		const retained = { ...float(0.5, 0.5), mode: 'constant', expression: 'old' } as ParamDescriptor;
		expect(isModified(retained)).toBe(false);
	});
});

describe('the source set', () => {
	it('starts as every mode, which takes nothing away', () => {
		expect(narrowing(SHOW_ALL)).toBe(false);
		expect(admits(SHOW_ALL, float(0.5, 0.5), NONE)).toBe(true);
		expect(admits(SHOW_ALL, expr('t'), NONE)).toBe(true);
		expect(admits(SHOW_ALL, ref('lfo.out'), NONE)).toBe(true);
	});

	it('flips one mode at a time, and says so', () => {
		const off = toggleSource(SHOW_ALL, 'constant');
		expect(off.sources).toEqual(['expression', 'reference']);
		expect(narrowing(off)).toBe(true);
		expect(toggleSource(off, 'constant').sources.sort()).toEqual(SHOW_ALL.sources.slice().sort());
	});

	it('admits only the modes still ticked', () => {
		const mapped = F({ sources: ['expression', 'reference'] });
		expect(admits(mapped, expr('t'), NONE)).toBe(true);
		expect(admits(mapped, ref('lfo.out'), NONE)).toBe(true);
		expect(admits(mapped, float(0.5, 0.5), NONE)).toBe(false);
	});

	it('admits nothing with every box cleared, rather than silently reopening', () => {
		expect(admits(F({ sources: [] }), float(0.5, 0.5), NONE)).toBe(false);
	});
});

describe('admits', () => {
	const groups = { common: { curve: expr('t'), frequency: float(0.7, 0.5) } };

	// AND, not OR: the source set says WHERE a value comes from and non-default says WHETHER it has
	// moved, so a reader after "the expression I have edited" narrows on both.
	it('ANDs the source set with the non-default list', () => {
		const both = F({ nonDefault: true, sources: ['expression'] });
		const list = listOf(groups);
		expect(admits(both, expr('t'), list, 'common', 'curve')).toBe(true);
		expect(admits(both, float(0.7, 0.5), list, 'common', 'frequency')).toBe(false);
	});

	it('leaves every source alone while non-default is off', () => {
		expect(admits(SHOW_ALL, float(0.5, 0.5), NONE)).toBe(true);
		expect(admits(F({ nonDefault: true }), float(0.5, 0.5), NONE)).toBe(false);
	});
});

describe('filteredRows', () => {
	const groups = {
		common: { frequency: float(0.7, 0.5), amplitude: float(0.5, 0.5) },
		shape: { curve: expr('t'), target: ref('lfo.out') }
	};

	// The tabs stay up under a filter, so the caller names the ONE group it is showing.
	it('answers only the groups it is given', () => {
		expect(filteredRows(groups, SHOW_ALL, NONE, ['shape']).map((r) => r.name)).toEqual([
			'curve',
			'target'
		]);
	});

	it('narrows one group to the sources still ticked', () => {
		const mapped = F({ sources: ['expression'] });
		expect(filteredRows(groups, mapped, NONE, ['shape']).map((r) => r.name)).toEqual(['curve']);
		expect(filteredRows(groups, mapped, NONE, ['common'])).toEqual([]);
	});

	it('spans every group when given none, which is what a search hands back', () => {
		expect(filteredRows(groups, SHOW_ALL, NONE).length).toBe(4);
	});

	it('narrows rows already gathered by a search', () => {
		const rows = filteredRows(groups, SHOW_ALL, NONE);
		expect(onlyAdmitted(rows, F({ sources: ['expression'] }), NONE).map((r) => r.name)).toEqual([
			'curve'
		]);
	});
});

describe('settleNonDefault', () => {
	const groups = {
		common: { frequency: float(0.7, 0.5), amplitude: float(0.5, 0.5) },
		shape: { curve: expr('t'), target: ref('lfo.out') }
	};

	// The list is the NODE's, because the clear beside it is: it says what pressing that would take
	// back, not what the fronted tab happens to hold.
	it('gathers across every group, whichever tab is fronted', () => {
		expect([...listOf(groups).keys()].sort()).toEqual([
			'common/frequency',
			'shape/curve',
			'shape/target'
		]);
	});

	/* The reported defect. A slider is dragged, and derived membership dropped its row the instant
	   the drag crossed the default — under the pointer holding it, sometimes back in a frame later
	   and sometimes not. So the list is STICKY: a param that has left its zero point stays until
	   that zero point moves. */
	it('keeps a param a drag has taken back onto its default', () => {
		const moved = { common: { frequency: float(0.7, 0.5) } };
		const list = listOf(moved);
		expect(list.has('common/frequency')).toBe(true);
		const onDefault = { common: { frequency: float(0.5, 0.5) } };
		expect(settleNonDefault(list, onDefault).has('common/frequency')).toBe(true);
		const past = { common: { frequency: float(0.3, 0.5) } };
		expect(settleNonDefault(list, past).has('common/frequency')).toBe(true);
	});

	it('answers the list it was given when nothing moved, so a value tick writes no state', () => {
		const list = listOf(groups);
		expect(settleNonDefault(list, groups)).toBe(list);
	});

	// The reported case: a preset moves hundreds of params, and Clear takes them back to zero.
	it('empties on the clear that moves every zero point, and refills as one moves again', () => {
		const list = listOf(groups);
		expect(list.size).toBe(3);
		const b: Baseline = {
			'common/frequency': { value: 0.7, mode: 'constant', expression: '', reference: '' },
			'common/amplitude': { value: 0.5, mode: 'constant', expression: '', reference: '' },
			'shape/curve': { value: 0.5, mode: 'expression', expression: 't', reference: '' },
			'shape/target': { value: 0.5, mode: 'reference', reference: 'lfo.out', expression: '' }
		};
		const cleared = settleNonDefault(list, groups, b);
		expect(cleared.size).toBe(0);
		const moved = { ...groups, common: { ...groups.common, frequency: float(0.9, 0.5) } };
		expect([...settleNonDefault(cleared, moved, b).keys()]).toEqual(['common/frequency']);
	});

	/* A clear's reply and its delta reach the browser on one socket, but the list must not depend on
	   which lands first: a value arriving in that window carries the OLD zero point, which is the one
	   every entry already stands on, so the list is untouched until the new baseline is what is read.
	*/
	it('is unmoved by a value that arrives before the new baseline does', () => {
		const list = listOf(groups);
		const ticked = { ...groups, common: { ...groups.common, frequency: float(0.72, 0.5) } };
		expect([...settleNonDefault(list, ticked).keys()].sort()).toEqual([...list.keys()].sort());
	});
});
