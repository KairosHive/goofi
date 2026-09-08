import { describe, it, expect } from 'vitest';
import { expressionFor, literalFor } from './paramSeed';
import type { ParamDescriptor } from '$lib/api/types';

const base = {
	doc: null,
	refreshable: false,
	expression: null,
	mode: 'constant',
	reference: null,
	triggers: false,
	error: null
} as const;

const param = (over: Partial<ParamDescriptor>): ParamDescriptor =>
	({ ...base, ...over }) as ParamDescriptor;

describe('the expression seed', () => {
	it('writes each value as the Python literal for it', () => {
		expect(literalFor(param({ type: 'float', value: 2.5 }))).toBe('2.5');
		expect(literalFor(param({ type: 'int', value: 3 }))).toBe('3');
		expect(literalFor(param({ type: 'bool', value: true }))).toBe('True');
		expect(literalFor(param({ type: 'bool', value: false }))).toBe('False');
		expect(literalFor(param({ type: 'string', value: 'sine' }))).toBe('"sine"');
	});

	it('seeds a pulse with False: it holds no value, and its source is a gate', () => {
		// `null` is what a JSON dump gives, and a Python expression of `null` never compiles.
		expect(literalFor(param({ type: 'pulse', value: null }))).toBe('False');
	});
});

// A dropped node offers one link in two spellings, so both must name the SAME producer output.
describe('the expression for a reference', () => {
	it('reads the slot behind .out, which is what an expression addresses', () => {
		expect(expressionFor('eeg.channels', 3)).toBe("nd('eeg').out.channels");
	});

	// `nd('lfo0').out.out` names the same slot and reads like a typo; the completions spell it bare.
	it('reads a one-output node bare', () => {
		expect(expressionFor('lfo0.out', 1)).toBe("nd('lfo0')");
	});
});
