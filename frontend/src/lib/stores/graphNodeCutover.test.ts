import { describe, it, expect } from 'vitest';
import { FakeControl } from '$lib/test/fakeControl';
import { seed } from '$lib/test/docSeed';
import { GraphStore } from './graph.svelte';
import { nodesMap } from '$lib/crdt/graphDoc';
import type { NodeTypeInfo } from '$lib/api/control';
import { slotView, isSlotExpanded } from '$lib/viewers/inlineView';
import { typeInfo } from '$lib/test/typeInfo';
import { numParam } from '$lib/test/param';
import { snapshot } from '$lib/test/snapshot';

/** A minimal hello/graph_replaced snapshot; `node_types` optionally carries the palette inline. */
const helloSnap = (node_types?: NodeTypeInfo[]) => snapshot({ node_types });

/** The catalog (list_nodes) the manager provides — the static per-type descriptor source. */
function catalog(): NodeTypeInfo[] {
	return [
		typeInfo({
			type: 'signal:Oscillator',
			doc: 'A generator',
			input_slots: { in: 'ARRAY' },
			output_slots: { out: 'ARRAY' },
			params: {
				common: {
					max_frequency: numParam({ value: 30, default: 30, vmax: 1000 })
				}
			}
		})
	];
}


describe('node-identity read cutover — nodes built from the doc when the catalog is present', () => {
	it('assembles a node from doc identity + catalog descriptors', () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		g.nodeTypes = catalog(); // catalog present → the doc becomes authoritative for node identity
		d.node('n1', 'signal:Oscillator', 'osc0', [10, 20]);

		const n = g.nodeById('n1');
		expect(n, 'node exists purely from the doc — no node_added event').toBeDefined();
		expect([n!.type, n!.name, n!.pos]).toEqual(['signal:Oscillator', 'osc0', [10, 20]]);
		// Descriptors come from the catalog by type.
		expect(n!.doc).toBe('A generator');
		expect(n!.output_slots).toEqual({ out: 'ARRAY' });
		expect(n!.params.common.max_frequency.type).toBe('num');
		// No doc value written → the catalog default.
		expect(n!.params.common.max_frequency.value).toBe(30);
	});

	it('drops a node when it vanishes from the doc (mirror removal)', () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		g.nodeTypes = catalog();
		d.node('n1', 'signal:Oscillator', 'osc0', [0, 0]);
		expect(g.nodeById('n1')).toBeDefined();

		d.remove('nodes', 'n1');
		expect(g.nodeById('n1'), 'node removed from the doc → dropped from the store').toBeFalsy();
	});

	it('state_update merges runtime bits without clobbering the doc-owned value', () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		g.nodeTypes = catalog();
		d.node('n1', 'signal:Oscillator', 'osc0', [0, 0]);
		d.patch({ nodes: { n1: { params: { common: { max_frequency: { value: 55 } } } } } });

		// A state_update carries options and stage, never a value or a param error: params are
		// doc-owned, and an op's echo predates the re-evaluation of the source it moved.
		fc.emit({ event: 'state_update', payload: { node: 'n1', options: {}, stage: 'ready', error: null } });
		const n = g.nodeById('n1')!;
		expect(n.params.common.max_frequency.value).toBe(55); // doc value preserved
		expect(n.params.common.max_frequency.error, 'the echo is one evaluation behind').toBeNull();
		expect(n.stage).toBe('ready');

		// The live plane is what carries a failure, and what clears it: both maps are WHOLE, so the
		// tick that omits the param is the one that says the expression is working again.
		fc.emit({
			event: 'param_values',
			payload: { nodes: { n1: { values: {}, errors: { common: { max_frequency: 'NameError' } } } } }
		});
		expect(g.nodeById('n1')!.params.common.max_frequency.error).toBe('NameError');
		fc.emit({ event: 'param_values', payload: { nodes: { n1: { values: {}, errors: {} } } } });
		expect(
			g.nodeById('n1')!.params.common.max_frequency.error,
			'a failure that cleared with no op behind it still reaches the replica'
		).toBeNull();
	});

	it('an unknown type (missing from the catalog) still renders identity + pos', () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		g.nodeTypes = catalog(); // does NOT contain "Mystery"
		d.node('n2', 'Mystery', 'mystery0', [5, 6]);

		const n = g.nodeById('n2');
		expect(n).toBeDefined();
		expect([n!.name, n!.pos]).toEqual(['mystery0', [5, 6]]);
		// Fallback: no descriptors, but the node still exists (doesn't crash the reconcile).
		expect(n!.input_slots).toEqual({});
		expect(n!.output_slots).toEqual({});
	});
});

describe('expression live value survives a doc rebuild', () => {
	it('an expression param keeps its param_values live value across an unrelated doc rebuild', () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		g.nodeTypes = catalog();
		d.node('n1', 'signal:Oscillator', 'osc0', [0, 0]);
		// Committed leaf value 99 + an ENABLED expression binding: the displayed value should track
		// the LIVE evaluation, not this committed literal.
		d.patch({
			nodes: {
				n1: {
					params: {
						common: {
							max_frequency: { value: 99, mode: 'expression', expression: "nd('lfo')" }
						}
					}
				}
			}
		});

		// A param_values event delivers the live evaluated value (7) — never written to the doc.
		fc.emit({ event: 'param_values', payload: { nodes: { n1: { values: { common: { max_frequency: 7 } }, errors: {} } } } });
		expect(g.nodeById('n1')!.params.common.max_frequency.value).toBe(7);

		// An unrelated doc change rebuilds every node from the doc. The live value must NOT revert to
		// the committed leaf (99) — the retired fallback path guarded this; the doc path must too.
		d.node('n2', 'signal:Oscillator', 'osc1', [1, 1]);
		expect(
			g.nodeById('n1')!.params.common.max_frequency.value,
			'live expression value preserved across the doc rebuild'
		).toBe(7);

		// The map is the node's WHOLE live state: one that no longer names the param withdraws its
		// value, and the committed literal shows again — a re-pointed reference leaves exactly this.
		fc.emit({ event: 'param_values', payload: { nodes: { n1: { values: {}, errors: {} } } } });
		expect(g.nodeById('n1')!.params.common.max_frequency.value, 'withdrawn').toBe(99);
	});
});

describe('catalog-in-hello — the palette rides on the snapshot, no async list_nodes', () => {
	it('a hello carrying node_types sets the catalog synchronously (doc-authoritative from render 1)', () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		fc.emit({ event: 'hello', payload: helloSnap(catalog()) });
		// The catalog is in hand immediately — the doc becomes authoritative with no fallback window.
		expect(g.nodeTypes?.length).toBe(1);
		// …and no async round-trip was issued for it.
		expect(fc.recordedCalls().some((c) => c.op === 'library list')).toBe(false);
	});

	it('a load is settled only once the replica reaches the version the snapshot names', () => {
		// The snapshot-first order: a fit armed on the epoch would frame the OUTGOING nodes.
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		fc.emit({ event: 'hello', payload: helloSnap(catalog()) });
		const d = seed(fc).node('n1', 'signal:Oscillator', 'osc0', [0, 0]);
		expect(g.loadSettled).toBe(true);

		const snap = { ...helloSnap(), doc_version: d.version + 1 };
		fc.emit({ event: 'graph_replaced', payload: snap });
		expect(g.loadSettled, 'the epoch moved, the document has not').toBe(false);
		d.node('n2', 'signal:Oscillator', 'osc1', [10, 10]);
		expect(g.loadSettled).toBe(true);
	});
});

describe('inline viewer state — the document is the one holder, so it follows the document', () => {
	/** The blob as the projection writes it: a JSON STRING leaf under the node. */
	const blob = (v: Record<string, unknown>) => ({ viewers: v });

	it('a peer’s collapse and kind land in this tab too', () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		g.nodeTypes = catalog();
		d.node('n1', 'signal:Oscillator', 'osc0', [0, 0], blob({ out: { collapsed: true, kind: 'image' } }));
		expect(isSlotExpanded(g.nodeById('n1'), 'out'), 'the saved collapse is what we draw').toBe(false);
		expect(slotView(g.nodeById('n1'), 'out').kind).toBe('image');

		// A SECOND tab expands the slot and picks another viewer. It reaches us as an ordinary doc
		// patch on a node we already hold — the case a client-side copy, seeded once per new uid,
		// never hears about: the two tabs then disagree, and whichever edits next overwrites the peer.
		d.patch({ nodes: { n1: blob({ out: { collapsed: false, kind: 'brain' } }) } });
		expect(isSlotExpanded(g.nodeById('n1'), 'out'), 'the peer’s expand converges').toBe(true);
		expect(slotView(g.nodeById('n1'), 'out').kind, 'and so does the peer’s kind').toBe('brain');
	});

	it('an in-session load onto a re-used uid draws the loaded patch, not the outgoing one', () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		g.nodeTypes = [...catalog(), { ...catalog()[0], type: 'signal:Buffer' }];
		fc.emit({ event: 'hello', payload: helloSnap() });
		const d = seed(fc);
		d.node('n1', 'signal:Oscillator', 'osc0', [0, 0], blob({ out: { collapsed: true, kind: 'image' } }));
		expect(isSlotExpanded(g.nodeById('n1'), 'out')).toBe(false);

		// A patch is loaded INTO the running session: same backend (so no generation reset), and it
		// restores the uids it was saved with — so `n1` is a SURVIVOR carrying a different node.
		fc.emit({ event: 'graph_replaced', payload: helloSnap() });
		d.push({ nodes: { n1: { type: 'signal:Buffer', name: 'buf0', pos: [0, 0] } }, links: {}, instances: {}, variables: {}, arrangement: {} });

		expect(g.nodeById('n1')!.type, 'precondition: the uid now carries the loaded node').toBe('signal:Buffer');
		expect(isSlotExpanded(g.nodeById('n1'), 'out'), 'the outgoing collapse did not stick').toBe(true);
		expect(slotView(g.nodeById('n1'), 'out').kind, 'nor the outgoing kind').toBeUndefined();
	});

	it('a write carries the whole blob, touches one slot, and invents no kind', () => {
		const fc = new FakeControl();
		const g = new GraphStore(fc);
		const d = seed(fc);
		g.nodeTypes = [{ ...catalog()[0], output_slots: { out: 'ARRAY', sig: 'ARRAY' } }];
		// `gone` is a slot the saved blob names and the node no longer has — a node file that lost an
		// output since the patch was written. It must not block an edit to a slot that does exist.
		d.node('n1', 'signal:Oscillator', 'osc0', [0, 0], blob({ out: { kind: 'image' }, sig: { collapsed: true }, gone: { collapsed: true } }));

		g.setSlotView('n1', 'sig', { collapsed: false });
		const call = fc.recordedCalls().find((c) => c.op === 'node edit');
		expect(call!.payload, 'only the slot edited travels — the manager merges the rest').toEqual({
			node: 'n1',
			viewer: [{ slot: 'sig', collapsed: false }]
		});
		// Nothing changed until the manager answers — the document is the holder, not this store.
		expect(isSlotExpanded(g.nodeById('n1'), 'sig')).toBe(false);
		d.patch({
			nodes: { n1: blob({ out: { kind: 'image' }, sig: { collapsed: false }, gone: { collapsed: true } }) }
		});
		expect(isSlotExpanded(g.nodeById('n1'), 'sig')).toBe(true);
	});
});
