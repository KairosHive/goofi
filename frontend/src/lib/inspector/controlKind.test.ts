import { describe, it, expect } from 'vitest';
import { controlKind } from './controlKind';
import type {
	BaseParam,
	FloatParam,
	IntParam,
	BoolParam,
	StringParam,
	PulseParam,
	UnknownParam
} from '$lib/api/types';

// The pure descriptor → control discriminant (spec §2, D-N2), over the TYPE alone: a pulse is a
// button, numeric = float|int, a bool is a toggle, a string with options OR that is refreshable is a
// select (an empty-but-refreshable list still gets a dropdown so its ⟳ re-scan survives), a plain
// string is text, anything else is unknown. The MODE decides nothing here — a row wears its own
// control in every mode, disabled where a source drives it — and the source's own editor is the
// second row, which is ParamField's business. Kept pure so that switch is thin and this is the SSOT.
const base: Omit<BaseParam, 'value'> = {
	default: null,
	doc: null,
	refreshable: false,
	expression: null,
	mode: 'constant',
	reference: null,
	triggers: false,
	error: null
};

const floatParam = (over: Partial<FloatParam> = {}): FloatParam => ({
	...base,
	type: 'float',
	value: 0,
	default: 0,
	vmin: 0,
	vmax: 1,
	...over
});
const intParam = (over: Partial<IntParam> = {}): IntParam => ({
	...base,
	type: 'int',
	value: 0,
	default: 0,
	vmin: 0,
	vmax: 10,
	...over
});
const boolParam = (over: Partial<BoolParam> = {}): BoolParam => ({
	...base,
	type: 'bool',
	value: false,
	default: false,
	...over
});
const stringParam = (over: Partial<StringParam> = {}): StringParam => ({
	...base,
	type: 'string',
	value: '',
	default: '',
	options: null,
	...over
});
const pulseParam = (over: Partial<PulseParam> = {}): PulseParam => ({
	...base,
	type: 'pulse',
	value: null,
	default: null,
	...over
});
const unknownParam = (over: Partial<UnknownParam> = {}): UnknownParam => ({
	...base,
	type: 'unknown',
	value: null,
	default: null,
	...over
});

describe('controlKind', () => {
	it('maps float and int to numeric', () => {
		expect(controlKind(floatParam())).toBe('numeric');
		expect(controlKind(intParam())).toBe('numeric');
	});

	it('maps a string with a non-empty options list to select', () => {
		expect(controlKind(stringParam({ options: ['a'] }))).toBe('select');
	});

	it('maps a refreshable string with an EMPTY options list to select (⟳ re-scan survives)', () => {
		expect(controlKind(stringParam({ options: [], refreshable: true }))).toBe('select');
	});

	it('maps a refreshable string with null options to select', () => {
		expect(controlKind(stringParam({ options: null, refreshable: true }))).toBe('select');
	});

	it('maps a plain string (no options, not refreshable) to text', () => {
		expect(controlKind(stringParam({ options: [], refreshable: false }))).toBe('text');
		expect(controlKind(stringParam({ options: null, refreshable: false }))).toBe('text');
	});

	it('maps a pulse to its own kind (a button, and no value)', () => {
		expect(controlKind(pulseParam())).toBe('pulse');
	});

	it('keeps a pulse a pulse in reference mode, so the button stays and the chips show', () => {
		expect(controlKind(pulseParam({ mode: 'reference', reference: 'clock.out' }))).toBe('pulse');
	});

	/* The mode used to win here, and the driven row rendered the source's editor INSTEAD of the
	   param's control. The control is what a reader recognises a param by, so it stays and reads out
	   what the source produces — which leaves this mapping a function of the type alone. */
	it('gives a driven param the very control its type gets, whatever drives it', () => {
		expect(controlKind(floatParam({ mode: 'expression' }))).toBe('numeric');
		expect(controlKind(intParam({ mode: 'reference' }))).toBe('numeric');
		expect(controlKind(boolParam({ mode: 'expression' }))).toBe('toggle');
		expect(controlKind(stringParam({ options: ['a'], mode: 'reference' }))).toBe('select');
		expect(controlKind(stringParam({ mode: 'expression' }))).toBe('text');
		expect(controlKind(unknownParam({ mode: 'expression' }))).toBe('unknown');
	});

	it('maps an unknown param to unknown', () => {
		expect(controlKind(unknownParam())).toBe('unknown');
	});

});
