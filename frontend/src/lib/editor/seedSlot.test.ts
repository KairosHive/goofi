import { describe, it, expect } from 'vitest';
import { seedSlot } from './seedSlot';
import { typeInfo } from '$lib/test/typeInfo';

const type = (input_slots: Record<string, string>, output_slots: Record<string, string>) =>
	typeInfo({ input_slots, output_slots });
const seed = (side: 'source' | 'target', dtype: string) =>
	({ node: 'n', slot: 's', side, dtype }) as Parameters<typeof seedSlot>[0];

describe('the seed rule is the manager\'s link rule', () => {
	it('an audio output seeds an audio input alone: the tap is a node, not a cable', () => {
		expect(seedSlot(seed('source', 'AUDIO'), type({ a: 'AUDIO' }, {}))).toBe('a');
		expect(seedSlot(seed('source', 'AUDIO'), type({ x: 'ARRAY' }, {}))).toBeUndefined();
		expect(seedSlot(seed('source', 'TEXTURE'), type({ x: 'ARRAY', t: 'TEXTURE' }, {}))).toBe('t');
	});
	it('an input takes its own kind only', () => {
		expect(seedSlot(seed('target', 'AUDIO'), type({}, { out: 'ARRAY' }))).toBeUndefined();
		expect(seedSlot(seed('target', 'ARRAY'), type({}, { out: 'AUDIO' }))).toBeUndefined();
		expect(seedSlot(seed('target', 'ARRAY'), type({}, { out: 'ARRAY' }))).toBe('out');
	});
});
