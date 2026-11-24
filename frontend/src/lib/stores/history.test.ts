import { describe, it, expect, beforeEach } from 'vitest';
import { history } from './history.svelte';
import { notify } from './notify.svelte';
import { FakeControl } from '$lib/test/fakeControl';
import { GraphStore } from './graph.svelte';
import { flash } from './flash.svelte';
import { captureNavContext, type NavContext } from './navContext';
import { historyFeed } from '$lib/api/control';
import { seed } from '$lib/test/docSeed';
import { typeInfo } from '$lib/test/typeInfo';
import { workspace } from 'panelty';

// The MANAGER owns the history: every write is a step there, and this store mirrors the labels its
// replies put on top. The fake control plays the manager's half, one entry per write.
function booted(): { fc: FakeControl; g: GraphStore } {
	const fc = new FakeControl();
	const g = new GraphStore(fc);
	history().configure(() => fc, () => g);
	return { fc, g };
}

describe('HistoryStore — a mirror of the manager', () => {
	beforeEach(() => history().reset());

	it('starts empty: canUndo/canRedo false, labels null', () => {
		const h = history();
		expect([h.canUndo, h.canRedo, h.undoLabel, h.redoLabel]).toEqual([false, false, null, null]);
	});

	it('a write reply carries the labels, and a later write clears the redo', async () => {
		const { g } = booted();
		await g.removeNode('a');
		expect(history().canUndo).toBe(true);
		expect(history().undoLabel).toBe('node remove');
		await history().undo();
		expect(history().canRedo).toBe(true);
		await g.removeNode('b');
		expect(history().canRedo).toBe(false);
		expect(history().undoLabel).toBe('node remove');
	});

	it('reset() forgets the labels', async () => {
		const { g } = booted();
		await g.removeNode('a');
		history().reset();
		expect([history().canUndo, history().canRedo, history().undoLabel]).toEqual([false, false, null]);
	});
});

describe('HistoryStore — re-entrancy (report B13: held Ctrl+Z)', () => {
	beforeEach(() => history().reset());

	const flips = (fc: FakeControl, op: string) => fc.recordedCalls().filter((c) => c.op === op);

	it('undo() fired twice before the first settles reaches the manager exactly once', async () => {
		const { fc, g } = booted();
		await g.removeNode('a');
		const p1 = history().undo();
		const p2 = history().undo();
		await Promise.all([p1, p2]);
		expect(flips(fc, 'undo')).toHaveLength(1);
		expect(history().canUndo).toBe(false);
		expect(history().canRedo).toBe(true);
		await history().redo();
		expect(flips(fc, 'redo')).toHaveLength(1);
		expect(history().canRedo).toBe(false);
		expect(history().canUndo).toBe(true);
	});
});

describe('HistoryStore — the failure surface (#9)', () => {
	beforeEach(() => {
		history().reset();
		notify().clear();
	});

	it('a refused flip raises the failure on the shared toast channel and keeps the step', async () => {
		const { fc, g } = booted();
		await g.removeNode('a');
		fc.failNext('undo');
		await history().undo();
		expect(notify().message).toBe('Undo failed: fake control: undo failed');
		expect(history().canUndo).toBe(true);
	});

	/** The reset drops the LABELS, not the alarm: the channel is shared, so a line this store never
	 * raised (a failed save, say) is not its to take down. */
	it('reset() leaves a showing toast alone', () => {
		notify().raise('Save failed: Permission denied');
		history().reset();
		expect(notify().message).toBe('Save failed: Permission denied');
	});
});

describe('HistoryStore — a transaction is one step', () => {
	beforeEach(() => history().reset());

	it('every write handed the step rides its token, so the manager merges them under the label', async () => {
		const { fc, g } = booted();
		await history().transaction('Move 2 nodes', async (step) => {
			await g.removeNode('a', step);
			await g.removeNode('b', step);
		});
		expect(fc.undoStack).toHaveLength(1);
		expect(history().undoLabel).toBe('Move 2 nodes');
		const groups = fc.recordedCalls().length;
		await g.removeNode('c');
		expect(fc.undoStack, 'a write after the transaction is a step of its own').toHaveLength(2);
		expect(fc.recordedCalls().length).toBe(groups + 1);
	});

	it('a nested transaction rides the outer one', async () => {
		const { fc, g } = booted();
		await history().transaction('Outer', async (step) => {
			await g.removeNode('a', step);
			await history().transaction('Inner', (inner) => g.removeNode('b', inner), step);
		});
		expect(fc.undoStack.map((e) => e.label)).toEqual(['Outer']);
	});

	it('a thrown transaction leaves what landed before the throw as one step', async () => {
		const { fc, g } = booted();
		await expect(
			history().transaction('Add + boom', async (step) => {
				await g.removeNode('a', step);
				throw new Error('boom');
			})
		).rejects.toThrow('boom');
		await g.removeNode('b');
		expect(fc.undoStack.map((e) => e.label)).toEqual(['Add + boom', 'node remove']);
	});
});

describe('HistoryStore — a flip pulses what it restored', () => {
	beforeEach(() => history().reset());

	it('pulses the selected nodes that are on the canvas, and skips the ones that are not', async () => {
		const { fc, g } = booted();
		const d = seed(fc);
		g.nodeTypes = [typeInfo({ type: 'Oscillator', input_slots: {}, output_slots: { out: 'ARRAY' }, params: {} })];
		// uid and display name kept distinct, so a lookup that confuses the two is caught.
		d.node('uf_present', 'Oscillator', 'display-present', [0, 0]);
		const ctx: NavContext = {
			activeWorkspaceId: workspace().state.activeWorkspaceId,
			activePanelId: null,
			enteredPath: {},
			selection: { p: { nodes: ['uf_present', 'uf_absent'], edges: [] } }
		};
		historyFeed.context = () => ctx;
		try {
			await g.removeNode('x');
		} finally {
			historyFeed.context = captureNavContext;
		}
		await history().undo();

		expect(flash().active('uf_present')).toBe(true);
		expect(flash().active('uf_absent')).toBe(false); // not in the graph — no flash, no throw
	});
});
