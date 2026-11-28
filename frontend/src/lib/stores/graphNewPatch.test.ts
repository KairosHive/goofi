import { describe, it, expect, beforeEach } from 'vitest';
import { FakeControl } from '$lib/test/fakeControl';
import { GraphStore } from './graph.svelte';
import { history } from './history.svelte';
import { snapshot } from '$lib/test/snapshot';

/** `New` is a manager transaction. The client records no history entry (the manager clears its
 * history) and does not write the save path: the null path arrives on `graph_replaced`. */
describe('GraphStore.newPatch — the reset door', () => {
	beforeEach(() => history().reset());

	it('sends `new` with an empty payload and records no undo step', async () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		fc.emit({ event: 'hello', payload: snapshot({ save_path: '/patches/a.gfi' }) });
		expect(g.savePath).toBe('/patches/a.gfi');

		await g.newPatch();

		// `list_nodes` is the boot catalog fetch a hello with no `node_types` triggers — not ours.
		const mine = fc.recordedCalls().filter((c) => c.op !== 'library list');
		expect(mine).toEqual([{ op: 'session new', payload: {} }]);
		expect(history().canUndo, 'a New is not undoable — the manager dropped its history').toBe(
			false
		);
	});

	it('takes the patch name down off the snapshot, with no `save_path_changed` to help', async () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		fc.emit({ event: 'hello', payload: snapshot({ save_path: '/patches/a.gfi' }) });

		await g.newPatch();
		fc.emit({ event: 'graph_replaced', payload: snapshot() });

		expect(g.savePath).toBe(null);
		expect(g.unsavedChanges).toBe(false);
	});
});
