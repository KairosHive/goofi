import { describe, expect, it } from 'vitest';
import type { ParamDescriptor } from '$lib/api/types';
import {
	admits,
	counts,
	filteredRows,
	isExpression,
	isModified,
	isReference,
	onlyAdmitted,
	type Baseline,
	type Filters
} from './paramTouched';

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

const F = (f: Partial<Filters>): Filters => ({
	touched: false,
	expression: false,
	reference: false,
	...f
});

describe('isModified', () => {
	it('is false for a param still sitting on its declared default', () => {
		expect(isModified(float(0.5, 0.5))).toBe(false);
	});

	it('is true once the value has moved — which is how a knob turned in a plugin window shows up', () => {
		expect(isModified(float(0.7, 0.5))).toBe(true);
	});

	// Being DRIVEN is no longer "touched" on its own: that is the Expression filter's question, and
	// folding it in here made "what did I change" and "what is driven" one question when they are two.
	it('is true for a newly bound expression, because binding one IS a change', () => {
		expect(isModified(expr('t'))).toBe(true);
	});

	it('is false for a pulse, which holds no value to compare', () => {
		const pulse = { ...base, type: 'pulse', value: null, default: null } as ParamDescriptor;
		expect(isModified(pulse)).toBe(false);
	});

	it('treats a string param off its default as touched', () => {
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

	// A mapping survives Clear as a MAPPING — it keeps driving and keeps its own filter — but it
	// stops counting as "changed", which is what makes the touched list quiet again.
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

describe('the expression and reference filters', () => {
	it('name the active source and nothing else', () => {
		expect(isExpression(expr('t'))).toBe(true);
		expect(isExpression(ref('lfo.out'))).toBe(false);
		expect(isReference(ref('lfo.out'))).toBe(true);
		expect(isReference(expr('t'))).toBe(false);
		expect(isExpression(float(0.5, 0.5))).toBe(false);
		expect(isReference(float(0.5, 0.5))).toBe(false);
	});

	// A mapping is worth seeing whether or not it has moved since the zero point — which is the
	// reason these are filters of their own rather than a corner of "touched".
	it('find a mapping that a clear has made untouched', () => {
		const b: Baseline = { 'common/frequency': { value: 0.5, mode: 'expression', expression: 't', reference: '' } };
		const d = expr('t');
		expect(isModified(d, b, 'common', 'frequency')).toBe(false);
		expect(admits(F({ expression: true }), d, b, 'common', 'frequency')).toBe(true);
	});
});

describe('admits', () => {
	it('admits everything when no filter is on', () => {
		expect(admits(F({}), float(0.5, 0.5))).toBe(true);
	});

	// OR, not AND: each filter names a set worth seeing, and a reader who wants "what I changed and
	// what I mapped" gets the union by turning both on.
	it('ORs the filters that are on', () => {
		const moved = float(0.7, 0.5);
		const mapped = ref('lfo.out');
		const both = F({ touched: true, reference: true });
		expect(admits(both, moved)).toBe(true);
		expect(admits(both, mapped)).toBe(true);
		expect(admits(F({ reference: true }), moved)).toBe(false);
	});
});

describe('filteredRows', () => {
	const groups = {
		common: { frequency: float(0.7, 0.5), amplitude: float(0.5, 0.5) },
		shape: { curve: expr('t'), target: ref('lfo.out') }
	};

	it('spans every group, because which tab a knob was filed under is what the reader does not know', () => {
		const rows = filteredRows(groups, F({ touched: true }));
		expect(rows.map((r) => r.group + '/' + r.name).sort()).toEqual([
			'common/frequency',
			'shape/curve',
			'shape/target'
		]);
	});

	it('narrows to one source when only that filter is on', () => {
		expect(filteredRows(groups, F({ expression: true })).map((r) => r.name)).toEqual(['curve']);
		expect(filteredRows(groups, F({ reference: true })).map((r) => r.name)).toEqual(['target']);
	});

	it('returns every param when nothing is filtered', () => {
		expect(filteredRows(groups, F({})).length).toBe(4);
	});

	it('narrows rows already gathered by a search', () => {
		const rows = filteredRows(groups, F({}));
		expect(onlyAdmitted(rows, F({ expression: true })).map((r) => r.name)).toEqual(['curve']);
	});
});

describe('counts', () => {
	it('counts each filter separately, across every group', () => {
		const groups = {
			common: { frequency: float(0.7, 0.5), amplitude: float(0.5, 0.5) },
			shape: { curve: expr('t'), target: ref('lfo.out') }
		};
		expect(counts(groups)).toEqual({ touched: 3, expression: 1, reference: 1 });
	});

	// The reported case: a preset moves hundreds of params, Clear takes them back to zero, and the
	// mappings are still findable by their own filters.
	it('drops the touched count to nothing after a clear, leaving the mappings countable', () => {
		const groups = { common: { a: float(0.7, 0.5), b: expr('t') } };
		const b: Baseline = {
			'common/a': { value: 0.7, mode: 'constant', expression: '', reference: '' },
			'common/b': { value: 0.5, mode: 'expression', expression: 't', reference: '' }
		};
		expect(counts(groups, b)).toEqual({ touched: 0, expression: 1, reference: 0 });
	});
});
