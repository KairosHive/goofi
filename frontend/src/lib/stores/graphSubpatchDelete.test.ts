import { describe, it, expect, beforeEach } from 'vitest';
import { FakeControl } from '$lib/test/fakeControl';
import { seed, type DocSeed } from '$lib/test/docSeed';
import { GraphStore } from './graph.svelte';
import { history } from './history.svelte';
import { SCOPE_TYPE } from '$lib/api/vocab';
import type { NodeTypeInfo, GraphSnapshot } from '$lib/api/control';
import { typeInfo } from '$lib/test/typeInfo';

/** Minimal catalog — its presence flips the store to doc-authoritative identity. */
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

/** A hello/graph_replaced snapshot — session frame + runtime overlay only (structure is the doc's). */
function snapshot(): GraphSnapshot {
	return {
		runtime: {},
		save_path: null,
		unsaved_changes: false,
		instance_id: 'sess1',
		doc_version: 0,
		viewpoint: null
	} as never;
}

/** A store holding a collapsed sub-patch instance `sub` (one member `m1`). */
function withInstance(): { fc: FakeControl; g: GraphStore; d: DocSeed } {
	const fc = new FakeControl();
	const g = new GraphStore(fc);
	g.nodeTypes = catalog();
	// Seed as the manager sends it — the scope forest's single source.
	const d = seed(fc).patch({
		nodes: {
			m1: { type: 'Buffer', name: 'buffer0', pos: [0, 0], scope: 'sub' },
			sub: { type: SCOPE_TYPE, name: 'subpatch0', pos: [0, 0] }
		}
	});
	return { fc, g, d };
}

describe('a wholesale load resets the client history (lockstep with the manager)', () => {
	beforeEach(() => history().reset());

	it('graph_replaced (in-session load) drops pre-load undo steps', async () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		// Establish the session, then record a pre-load graph step.
		fc.emit({ event: 'hello', payload: snapshot() });
		await g.removeNode('x');
		expect(history().canUndo).toBe(true);

		// A load replaces the graph in the SAME backend session — the manager cleared its command
		// history (`load`/`new` → CommandHistory::clear), so the client's stale entries would pop
		// mismatched. graph_replaced must reset the client history too.
		fc.emit({ event: 'graph_replaced', payload: snapshot() });
		expect(history().canUndo).toBe(false);
	});
});

describe('deleting a collapsed sub-patch instance is undoable (manager owns the subtree capture)', () => {
	beforeEach(() => history().reset());

	it('undo is one `undo` op: the manager restores the subtree', async () => {
		const { fc, g } = withInstance();
		history().configure(() => fc, () => g);

		await g.removeNode('sub');
		const before = fc.recordedCalls().length;
		await history().undo();

		// The manager holds the inverse of the whole subtree, so the client sends this op alone.
		expect(fc.recordedCalls().slice(before).map((c) => c.op)).toEqual(['undo']);
		expect(history().canRedo).toBe(true);
	});

	it('a MIXED batch (instance + node) is ONE undo step delegating per child', async () => {
		// `sub` (holding member `m1`) plus a top-level node `n1`.
		const { fc, g, d } = withInstance();
		d.node('n1', 'Buffer', 'buffer1');
		history().configure(() => fc, () => g);

		await g.removeNodes(['n1', 'sub']);

		// Forward deletes both.
		const removed = fc
			.recordedCalls()
			.filter((c) => c.op === 'node remove')
			.map((c) => c.payload.node);
		expect(removed).toContain('n1');
		expect(removed).toContain('sub');
		// ONE undoable entry: both deletes carried the transaction's group token.
		expect(fc.undoStack).toHaveLength(1);
		expect(history().undoLabel).toBe('Delete 2 nodes');

		// Undo is one manager `undo` and nothing else; redo is its mirror.
		const before = fc.recordedCalls().length;
		await history().undo();
		expect(fc.recordedCalls().slice(before).map((c) => c.op)).toEqual(['undo']);
		expect(history().canRedo).toBe(true);

		const beforeRedo = fc.recordedCalls().length;
		await history().redo();
		expect(fc.recordedCalls().slice(beforeRedo).map((c) => c.op)).toEqual(['redo']);
		expect(history().canRedo).toBe(false);
		expect(history().canUndo).toBe(true);
	});
});
