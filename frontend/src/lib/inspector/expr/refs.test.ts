import { describe, it, expect } from 'vitest';
import { refNodes, refSlots, splitReference, wantedDtype } from './refs';
import type { ExprCatalogue } from './catalogue';

const cat: ExprCatalogue = {
	nodes: [
		{ name: 'osc', engine: 'signal', slots: [{ name: 'out', dtype: 'ARRAY' }], params: [] },
		{
			name: 'tagger',
			engine: 'signal',
			slots: [
				{ name: 'label', dtype: 'STRING' },
				{ name: 'count', dtype: 'ARRAY' }
			],
			params: []
		},
		{ name: 'sink', engine: null, slots: [], params: [] },
		{ name: 'synth', engine: 'audio', slots: [{ name: 'out', dtype: 'AUDIO' }], params: [] },
		{ name: 'gain', engine: 'audio', slots: [{ name: 'out', dtype: 'AUDIO' }], params: [] }
	],
	variables: []
};

describe('the reference picker offers only what the param may reference', () => {
	it('types by the param: a string reads a STRING output, everything else an ARRAY one', () => {
		expect(wantedDtype('string')).toBe('STRING');
		for (const t of ['float', 'int', 'bool', 'pulse', 'unknown']) expect(wantedDtype(t)).toBe('ARRAY');
	});

	it('lists the nodes with at least one output of the kind, naming those outputs', () => {
		// An audio output is not an array: a param reads one through `signal:AudioIn`, never directly.
		expect(refNodes(cat, 'ARRAY')).toEqual([
			{ label: 'osc', detail: 'out' },
			{ label: 'tagger', detail: 'count' }
		]);
		// …except on the audio plane itself, where a param reading an audio output is a plan edge.
		expect(refNodes(cat, 'ARRAY', 'gain').map((o) => o.label)).toEqual(['osc', 'tagger', 'synth', 'gain']);
		expect(refSlots(cat, 'synth', 'ARRAY', 'gain')).toEqual([{ label: 'out', detail: 'AUDIO' }]);
		expect(refSlots(cat, 'synth', 'ARRAY', 'osc')).toEqual([]);
		expect(refNodes(cat, 'STRING')).toEqual([{ label: 'tagger', detail: 'label' }]);
	});

	it("lists one node's outputs of the kind, and nothing for an unknown node", () => {
		expect(refSlots(cat, 'tagger', 'ARRAY')).toEqual([{ label: 'count', detail: 'ARRAY' }]);
		expect(refSlots(cat, 'nope', 'ARRAY')).toEqual([]);
	});

	it('splits node.slot at its one dot, and reads nothing into a malformed value', () => {
		expect(splitReference('osc.out')).toEqual(['osc', 'out']);
		expect(splitReference('osc')).toEqual(['', '']);
		expect(splitReference(null)).toEqual(['', '']);
	});
});
