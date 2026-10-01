import { describe, it, expect, beforeEach } from 'vitest';
import { commands } from './commands';
import { query } from './query';
import { FakeControl } from '$lib/test/fakeControl';
import { GraphStore } from '$lib/stores/graph.svelte';
import { history } from '$lib/stores/history.svelte';

describe('agent surface — undo/redo', () => {
	let g: GraphStore;
	beforeEach(() => {
		history().reset();
		const fc = new FakeControl();
		g = new GraphStore(fc);
		history().configure(() => fc, () => g);
	});

	it('query.canUndo / canRedo / undoLabel reflect the manager', async () => {
		expect(query.canUndo()).toBe(false);
		await g.removeNode('a');
		expect(query.canUndo()).toBe(true);
		expect(query.undoLabel()).toBe('node remove');
	});

	it('commands.undo / redo reach the manager', async () => {
		await g.removeNode('a');
		await commands.undo();
		expect(query.canUndo()).toBe(false);
		expect(query.canRedo()).toBe(true);
		await commands.redo();
		expect(query.canUndo()).toBe(true);
	});
});
