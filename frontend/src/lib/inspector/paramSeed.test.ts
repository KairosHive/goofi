import { describe, it, expect } from 'vitest';
import { literalFor } from './paramSeed';
import type { ParamDescriptor } from '$lib/api/types';

const base = {
	doc: null,
	refreshable: false,
	expression: null,
	mode: 'constant',
	triggers: false,
	error: null
} as const;

const param = (over: Partial<ParamDescriptor>): ParamDescriptor =>
	({ ...base, ...over }) as ParamDescriptor;

describe('the expression seed', () => {
	it('writes each value as the Python literal for it', () => {
		expect(literalFor(param({ type: 'num', value: 2.5 }))).toBe('2.5');
		expect(literalFor(param({ type: 'num', value: 3, int: true }))).toBe('3');
		expect(literalFor(param({ type: 'num', value: [0.5, 1] }))).toBe('[0.5, 1]');
		expect(literalFor(param({ type: 'bool', value: true }))).toBe('True');
		expect(literalFor(param({ type: 'bool', value: false }))).toBe('False');
		expect(literalFor(param({ type: 'string', value: 'sine' }))).toBe('"sine"');
	});

	it('seeds a pulse with False: it holds no value, and its source is a gate', () => {
		// `null` is what a JSON dump gives, and a Python expression of `null` never compiles.
		expect(literalFor(param({ type: 'pulse', value: null }))).toBe('False');
	});
});

