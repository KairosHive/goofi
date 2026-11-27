import { describe, it, expect } from 'vitest';
import { FakeControl } from '$lib/test/fakeControl';
import { seed } from '$lib/test/docSeed';
import { GraphStore } from './graph.svelte';
import { ROOT_ID } from '$lib/editor/subpatchScene';
import type { NodeTypeInfo } from '$lib/api/control';
import { typeInfo } from '$lib/test/typeInfo';

/** The catalog makes the doc authoritative for node + scope identity (see the node cutover). */
function catalog(): NodeTypeInfo[] {
	return [
		typeInfo({
			type: 'Buffer',
			input_slots: { in: 'ARRAY' },
			output_slots: { out: 'ARRAY' },
			params: {}
		})
	];
}

describe('root-as-scope: ROOT is the canvas, and no record is it', () => {
	it('a top-level record NAMES the root scope, and ROOT itself resolves to no node', () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		g.nodeTypes = catalog();
		seed(fc).node('m1', 'Buffer', 'buffer0');

		expect(g.nodeById('m1')!.scope).toBe(ROOT_ID); // a record with no `scope` is drawn at ROOT
		expect(g.nodeById(ROOT_ID)).toBe(null); // …and the sentinel names no record
	});

	it('restartNode respawns in place: one `node restart` op and no other', async () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);

		await g.restartNode('m1');

		// One op and no other: the manager respawns the node where it is.
		expect(fc.recordedCalls()).toEqual([{ op: 'node restart', payload: { node: 'm1' } }]);
	});
});
