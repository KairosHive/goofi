import { describe, it, expect, vi } from 'vitest';
import { FakeControl } from '$lib/test/fakeControl';
import { SyncClient } from './syncClient.svelte';
import { nodeView } from './graphDoc';

const OSC = { type: 'Oscillator', name: 'osc', pos: [0, 0] };

/** A document as the manager sends it, whole. */
const stateWith = (nodes: Record<string, unknown>) => ({
	nodes,
	links: {},
	instances: {},
	variables: {},
	arrangement: {}
});

function started(): { ctl: FakeControl; client: SyncClient } {
	const ctl = new FakeControl();
	const client = new SyncClient(ctl);
	client.start();
	return { ctl, client };
}

describe('SyncClient', () => {
	it('is seeded by the doc_state the manager sends unprompted — it asks for nothing', () => {
		const { ctl, client } = started();
		expect(client.synced).toBe(false);
		expect(ctl.recordedCalls().length, 'a replica never speaks: it is read-only').toBe(0);

		ctl.emit({ event: 'doc_state', payload: { v: 7, doc: stateWith({ '1': OSC }) } });
		expect(client.synced).toBe(true);
		expect(client.version).toBe(7);
		expect(nodeView(client.doc, '1')).toMatchObject({ type: 'Oscillator', name: 'osc' });
	});

	it('applies a delta onto the version it names', () => {
		const { ctl, client } = started();
		ctl.emit({ event: 'doc_state', payload: { v: 1, doc: stateWith({ '1': OSC }) } });
		ctl.emit({
			event: 'doc_patch',
			payload: { from: 1, v: 2, ops: [{ op: 'put', path: ['nodes', '2'], value: { type: 'Buffer', name: 'buf' } }] }
		});
		expect(client.version).toBe(2);
		expect(nodeView(client.doc, '2')).toMatchObject({ type: 'Buffer', name: 'buf' });
		expect(nodeView(client.doc, '1'), 'and leaves the untouched node alone').not.toBeNull();
	});

	it('a del REMOVES the path — and a put lands leaf by leaf', () => {
		// The half a delta format is easiest to get wrong. A replica that dropped deletes would keep
		// every removed node for ever and look perfectly healthy doing it.
		const { ctl, client } = started();
		ctl.emit({ event: 'doc_state', payload: { v: 1, doc: stateWith({ '1': OSC }) } });
		ctl.emit({ event: 'doc_patch', payload: { from: 1, v: 2, ops: [{ op: 'del', path: ['nodes', '1'] }] } });
		expect(nodeView(client.doc, '1')).toBeNull();
	});

	it('a whole-map put takes the manager key order, which a list shows', () => {
		const { ctl, client } = started();
		const vars = (names: string[]) => Object.fromEntries(names.map((n) => [n, { value: 1 }]));
		ctl.emit({ event: 'doc_state', payload: { v: 1, doc: { ...stateWith({}), variables: vars(['p.a', 'p.b']) } } });
		// A rename keeps its place: the manager sends the reordered map whole.
		const put = { op: 'put' as const, path: ['variables'], value: vars(['p.z', 'p.b']) };
		ctl.emit({ event: 'doc_patch', payload: { from: 1, v: 2, ops: [put] } });
		expect(Object.keys(client.doc.variables as object)).toEqual(['p.z', 'p.b']);
	});

	it('skips a stale delta in silence — the seed already carried it', () => {
		// The manager subscribes a socket BEFORE it snapshots the document, so a peer's edit landing
		// in that window is broadcast and then included in the snapshot too. Re-delivery is the
		// price of never losing one; a replica that treated it as a gap would stall on every
		// connection made while someone else was editing.
		const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
		const { ctl, client } = started();
		ctl.emit({ event: 'doc_state', payload: { v: 5, doc: stateWith({ '1': OSC }) } });
		ctl.emit({ event: 'doc_patch', payload: { from: 3, v: 4, ops: [{ op: 'del', path: ['nodes', '1'] }] } });
		expect(nodeView(client.doc, '1'), 'the already-applied delete was not replayed').not.toBeNull();
		expect(client.version).toBe(5);
		expect(warn, 'and it is not worth a word').not.toHaveBeenCalled();
		warn.mockRestore();
	});

	it('refuses a delta that reaches PAST this replica, and waits for a fresh doc_state', () => {
		// A gap can only mean the client fell behind the broadcast ring. Applying anyway would leave
		// a replica that looks healthy and is wrong, so it stops until the manager re-seeds it —
		// which the manager does on exactly that lag.
		const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
		const { ctl, client } = started();
		ctl.emit({ event: 'doc_state', payload: { v: 1, doc: stateWith({ '1': OSC }) } });
		ctl.emit({
			event: 'doc_patch',
			payload: { from: 5, v: 6, ops: [{ op: 'put', path: ['nodes', '9'], value: { type: 'Buffer', name: 'b' } }] }
		});
		expect(nodeView(client.doc, '9'), 'the out-of-order delta was not applied').toBeNull();
		expect(client.version, 'and the replica did not move').toBe(1);
		expect(warn).toHaveBeenCalled();

		ctl.emit({ event: 'doc_state', payload: { v: 6, doc: stateWith({ '9': OSC }) } });
		expect(client.version).toBe(6);
		expect(nodeView(client.doc, '9'), 'the re-seed healed it').not.toBeNull();
		warn.mockRestore();
	});

	it('reset() empties the replica so a new session cannot read the old one', () => {
		// A fresh engine mints uids from 1 again, so a surviving document would collide on reused
		// uids and its leaves would read as the new session's.
		const { ctl, client } = started();
		ctl.emit({ event: 'doc_state', payload: { v: 3, doc: stateWith({ '1': OSC }) } });
		client.reset();
		expect(client.synced).toBe(false);
		expect(nodeView(client.doc, '1')).toBeNull();

		ctl.emit({ event: 'doc_state', payload: { v: 1, doc: stateWith({ '1': { type: 'Buffer', name: 'b' } }) } });
		expect(nodeView(client.doc, '1')).toMatchObject({ type: 'Buffer' });
	});

	it('fires the change callback on a seed, on a delta and on a reset, naming what moved', () => {
		// A seed and a reset replace the whole document (`null`); a delta hands over its own ops,
		// so the store re-derives only the roots — and under `nodes`, the uids — they name.
		const { ctl, client } = started();
		const changes: unknown[] = [];
		client.onDocChange((ops) => changes.push(ops));
		const ops = [{ op: 'put' as const, path: ['nodes', '1'], value: { ...OSC, name: 'renamed' } }];
		ctl.emit({ event: 'doc_state', payload: { v: 1, doc: stateWith({ '1': OSC }) } });
		ctl.emit({ event: 'doc_patch', payload: { from: 1, v: 2, ops } });
		client.reset();
		expect(changes).toEqual([null, ops, null]);
	});

	it('stop() unsubscribes, so a later event no longer moves the replica', () => {
		const { ctl, client } = started();
		client.stop();
		ctl.emit({ event: 'doc_state', payload: { v: 1, doc: stateWith({ '1': OSC }) } });
		expect(client.synced).toBe(false);
	});
});
