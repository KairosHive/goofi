import { describe, it, expect } from 'vitest';
import { FakeControl } from '$lib/test/fakeControl';
import { seed } from '$lib/test/docSeed';
import { GraphStore } from './graph.svelte';
import { nodesMap } from '$lib/crdt/graphDoc';
import type { NodeTypeInfo } from '$lib/api/control';
import { typeInfo } from '$lib/test/typeInfo';
import { snapshot } from '$lib/test/snapshot';

/** The catalog (list_nodes) the manager provides — its presence flips the store to
 * doc-authoritative for node identity. */
function catalog(): NodeTypeInfo[] {
	return [
		typeInfo({
			type: 'PSD',
			input_slots: { data: 'ARRAY' },
			output_slots: { psd: 'ARRAY' },
			params: {}
		})
	];
}


describe('node lifecycle stage', () => {
	it('seeds stage from the doc and follows state_update', () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		g.nodeTypes = catalog();
		d.node('n1', 'PSD', 'psd0', [0, 0]);
		// The node's identity is doc-owned; its lifecycle stage is event-sourced. A node the client
		// has only just learned of has reported nothing yet, and `creating` is exactly what that
		// means — the add answers before the node's thread has said anything.
		expect(g.nodeById('n1')).toBeDefined();
		expect(g.nodeById('n1')?.stage).toBe('creating');

		fc.emit({
			event: 'state_update',
			payload: { node: 'n1', options: {}, stage: 'setup' }
		});
		expect(g.nodeById('n1')?.stage).toBe('setup');

		fc.emit({
			event: 'state_update',
			payload: { node: 'n1', options: {}, stage: 'ready' }
		});
		expect(g.nodeById('n1')?.stage).toBe('ready');
	});

	it('the snapshot overlay reaches a node before and after the doc holds it, and a doc change keeps it', () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		// The hello lands first and resets the replica: the overlay waits for a node yet to come.
		fc.emit({
			event: 'hello',
			payload: snapshot({ runtime: { n1: { stage: 'error', error: 'ImportError: no scipy', runtime: 'subprocess' } } })
		});
		const d = seed(fc);
		g.nodeTypes = catalog();
		d.node('n1', 'PSD', 'psd0', [0, 0]);
		const runtime = () => {
			const n = g.nodeById('n1')!;
			return [n.stage, n.error, n.runtime, n.stats];
		};
		fc.emit({ event: 'node_stats', payload: { stats: { n1: { updates_per_second: 12.4 } } } });
		const seeded = ['error', 'ImportError: no scipy', 'subprocess', { updates_per_second: 12.4 }];
		expect(runtime()).toEqual(seeded);

		// None of it is in the doc: a rename and a new node must not reset it.
		d.patch({ nodes: { n1: { name: 'renamed' } } });
		d.node('n2', 'PSD', 'psd1', [1, 1]);
		expect(g.nodeById('n1')?.name).toBe('renamed');
		expect(runtime()).toEqual(seeded);

		// …and a state_update, which carries no runtime at all, must not blank it either.
		fc.emit({ event: 'state_update', payload: { node: 'n1', options: {}, stage: 'error', error: 'GIL' } });
		expect(g.nodeById('n1')?.stage).toBe('error');
		expect(g.nodeById('n1')?.runtime).toBe('subprocess');

		// A reconnect and a load re-send the overlay for a node already here.
		fc.emit({
			event: 'hello',
			payload: snapshot({ runtime: { n1: { stage: 'ready', error: null, runtime: 'in-process' } } })
		});
		expect(runtime().slice(0, 3)).toEqual(['ready', null, 'in-process']);
		fc.emit({
			event: 'graph_replaced',
			payload: snapshot({ runtime: { n1: { stage: 'error', error: 'boom', runtime: 'in-process' } } })
		});
		expect(runtime().slice(0, 3)).toEqual(['error', 'boom', 'in-process']);
	});

	it('a node ADDED after connecting gets its runtime off the transition channel', () => {
		// The snapshot is sent on connect and on load, so a node dragged in afterwards appears in
		// none — its tier has to ride the stage transitions it is about to make anyway.
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		g.nodeTypes = catalog();
		d.node('n1', 'PSD', 'psd0', [0, 0]);
		expect(g.nodeById('n1')?.stage, 'born mid-session, ahead of any word from its thread').toBe(
			'creating'
		);
		expect(g.nodeById('n1')?.runtime, 'nothing has said where it runs yet').toBeUndefined();

		fc.emit({
			event: 'node_stage',
			payload: { node: 'n1', stage: 'ready', error: null, runtime: 'in-process' }
		});
		expect(g.nodeById('n1')?.runtime).toBe('in-process');

		// A GIL demotion moves the tier mid-session, and arrives the same way.
		fc.emit({
			event: 'node_stage',
			payload: { node: 'n1', stage: 'ready', error: 'GIL re-enabled', runtime: 'subprocess' }
		});
		expect(g.nodeById('n1')?.runtime).toBe('subprocess');

		// A stage event with no runtime at all must leave the known tier alone, not blank it.
		fc.emit({ event: 'node_stage', payload: { node: 'n1', stage: 'ready', error: null } });
		expect(g.nodeById('n1')?.runtime).toBe('subprocess');

		// The status worker's sweep can win the lock race and announce a node BEFORE the doc delta
		// that materializes it. The plane pushes each transition once, so the outrun report must
		// wait for the seed — dropped, a fast node wedges at the seed's `creating` guess for good.
		fc.emit({
			event: 'node_stage',
			payload: { node: 'n2', stage: 'ready', error: null, runtime: 'in-process' }
		});
		d.node('n2', 'PSD', 'psd1', [0, 0]);
		expect(g.nodeById('n2')?.stage, 'the outrun ready seeded the late node').toBe('ready');
		expect(g.nodeById('n2')?.runtime).toBe('in-process');
	});

	it('state_update carries the error and applies it (a healthy respawn clears the stale chip)', () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		g.nodeTypes = catalog();
		d.node('n1', 'PSD', 'psd0', [0, 0]);

		// a setup() failure rides the idempotent state plane
		fc.emit({
			event: 'state_update',
			payload: {
				node: 'n1',
				options: {},
				stage: 'setup',
				error: 'RuntimeError: setup boom'
			}
		});
		expect(g.nodeById('n1')?.error).toContain('setup boom');

		// a healthy respawn's state push carries error=null -> the chip clears
		fc.emit({
			event: 'state_update',
			payload: { node: 'n1', options: {}, stage: 'ready', error: null }
		});
		expect(g.nodeById('n1')?.error).toBe(null);
	});

	it('node_stage error is terminal and carries the traceback', () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		g.nodeTypes = catalog();
		d.node('n1', 'PSD', 'psd0', [0, 0]);

		fc.emit({
			event: 'node_stage',
			payload: { node: 'n1', stage: 'error', error: 'ModuleNotFoundError: torch' }
		});
		expect(g.nodeById('n1')?.stage).toBe('error');
		expect(g.nodeById('n1')?.error).toContain('ModuleNotFoundError');
	});
});
