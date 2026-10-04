import { describe, it, expect } from 'vitest';
import {
	nodesMap,
	linkViews,
	facadeFaces,
	variableViews,
	variableGroupLocks,
	effectiveLock,
	groupedVariables,
	isValidIdentifier,
	isValidName,
	arrangementTabs,
	type Doc
} from './graphDoc';
import { liveNode, type ViewSources } from './liveNode.svelte';

/** A document in the exact shape `goofi_bridge::projection` builds. */
function seedDoc(): Doc {
	return {
		nodes: {
			a: {
				type: 'Oscillator',
				name: 'osc0',
				pos: [10, 20],
				params: {
					common: { max_frequency: { value: 30 } },
					oscillator: {
						waveform: { value: 'sine', mode: 'expression', expression: "nd('lfo')" }
					}
				}
			},
			b: { type: 'Buffer', name: 'buf0' }
		},
		links: { 'a.out>b.data': { node_out: 'a', slot_out: 'out', node_in: 'b', slot_in: 'data' } },
		variables: {},
		arrangement: {}
	};
}

/** The production node reader, over `doc` alone: no catalog, face or runtime. */
const live = (doc: Doc, uid: string) => {
	const cx: ViewSources = { doc: () => doc, catalog: () => undefined, face: () => undefined, runtime: () => undefined };
	return liveNode(uid, cx);
};

describe('graphDoc readers', () => {
	it('reads node identity views', () => {
		const doc = seedDoc();
		expect(live(doc, 'a')).toMatchObject({
			uid: 'a', type: 'Oscillator', name: 'osc0', pos: [10, 20], scope: '__root__'
		});
		// A node with no pos defaults to [0,0], and one naming no scope is at the top level.
		expect(live(doc, 'b')).toMatchObject({
			uid: 'b', type: 'Buffer', name: 'buf0', pos: [0, 0], scope: '__root__'
		});
		expect(Object.keys(nodesMap(doc))).toEqual(['a', 'b']);
	});

	it('reads param values and expression sources', () => {
		const doc = seedDoc();
		const p = live(doc, 'a').params;
		expect(p.common?.max_frequency?.value).toBe(30);
		expect(p.oscillator?.waveform?.value).toBe('sine');
		expect(p.common?.nope?.value).toBeUndefined();
		expect(p.oscillator?.waveform?.mode).toBe('expression');
		expect(p.oscillator?.waveform?.expression).toBe("nd('lfo')");
		expect(p.common?.max_frequency?.mode).toBe('constant');
		expect(p.common?.max_frequency?.expression).toBeNull();
		// A node with no params → empty.
		expect(live(doc, 'b').params).toEqual({});
	});

	it('reads links', () => {
		const doc = seedDoc();
		expect(linkViews(doc)).toEqual([
			{ node_out: 'a', slot_out: 'out', node_in: 'b', slot_in: 'data' }
		]);
	});

	it('reads the sub-patch forest out of the one node map it is written in', () => {
		// The shape the projection writes: a facade and a port are node records, membership rides
		// each record as `scope`, and a port's inner wire is a link like any other.
		const base = seedDoc();
		const doc: Doc = {
			...base,
			nodes: {
				...(base.nodes as Record<string, unknown>),
				m1: { type: 'Buffer', name: 'm0', scope: 'i1' },
				i1: { type: 'SubPatch', name: 'subpatch0', pos: [5, 6] },
				p1: { type: 'OutArray', name: 'wave', pos: [1, 2], scope: 'i1' }
			},
			links: {
				...(base.links as Record<string, unknown>),
				'm1.out>p1.value': { node_out: 'm1', slot_out: 'out', node_in: 'p1', slot_in: 'value' }
			}
		};

		// ONE list, because the document is one map: leaf, facade and port alike, each carrying the
		// scope it is drawn in.
		expect(Object.keys(nodesMap(doc)).map((uid) => [uid, live(doc, uid).scope])).toEqual([
			['a', '__root__'],
			['b', '__root__'],
			['m1', 'i1'],
			['i1', '__root__'],
			['p1', 'i1']
		]);
		// A port IS the facade's slot: keyed by the port's stable uid, labelled with its name.
		expect(facadeFaces(doc).get('i1')).toEqual({
			input_slots: {},
			output_slots: { p1: 'ARRAY' },
			slot_labels: { p1: 'wave' },
			memberCount: 2
		});
		// A uid that names a LEAF has no face, however much it looks like a scope from outside.
		expect(facadeFaces(doc).has('a')).toBe(false);
	});

	it('an unwired port is a slot like any other — a facade with nothing behind it still has a face', () => {
		// The unwired state is the test: a port with no link naming it is present, named and
		// addressable, exactly as a leaf that was never connected is.
		const doc: Doc = {
			...seedDoc(),
			nodes: { i1: { type: 'SubPatch', name: 's' }, p1: { type: 'InArray', name: 'a', scope: 'i1' } },
			links: {}
		};
		expect(facadeFaces(doc).get('i1')).toEqual({
			input_slots: { p1: 'ARRAY' },
			output_slots: {},
			slot_labels: { p1: 'a' },
			memberCount: 1
		});
		// …and an EMPTY sub-patch is a node too, with a face that simply exposes nothing.
		expect(facadeFaces({ nodes: { i1: { type: 'SubPatch', name: 's' } } }).get('i1')).toEqual({
			input_slots: {},
			output_slots: {},
			slot_labels: {},
			memberCount: 0
		});
	});

	it('a wrongly-typed or absent root reads as empty rather than throwing', () => {
		// The manager is the sole author, so this can only mean the two ends have drifted — and a
		// half-drawn graph reports that better than a blank page does.
		expect(nodesMap({})).toEqual({});
		expect(linkViews({ links: 'not a map' })).toEqual([]);
		expect(variableViews({ variables: null })).toEqual([]);
		expect(facadeFaces({ nodes: 7 }).size).toBe(0);
	});
});

describe('graphDoc variables', () => {
	it('reads variable views (system-first, with the lock each carries)', () => {
		const doc: Doc = {
			...seedDoc(),
			variables: {
				'system.default_ufreq': { value: 30 },
				'system.goofi_home': { value: '/home/u/.goofi', lock: { value: true } },
				'patch.subject': { value: 'P07' },
				'mixer.gain': {
					value: 0.5,
					control: { kind: 'knob', min: 0, max: 1, x: 2, y: 1, w: 2, h: 2 }
				},
				// Every variable is `group.element`; one without a group is malformed and is skipped,
				// exactly as one whose value is no literal is.
				loose: { value: 1 }
			}
		};
		const free = { config: false, value: false };
		expect(variableViews(doc)).toEqual([
			{ name: 'system.default_ufreq', group: 'system', element: 'default_ufreq', value: 30, control: undefined, lock: free },
			{ name: 'system.goofi_home', group: 'system', element: 'goofi_home', value: '/home/u/.goofi', control: undefined, lock: { config: false, value: true } },
			{ name: 'patch.subject', group: 'patch', element: 'subject', value: 'P07', control: undefined, lock: free },
			{ name: 'mixer.gain', group: 'mixer', element: 'gain', value: 0.5, control: { kind: 'knob', min: 0, max: 1, x: 2, y: 1, w: 2, h: 2 }, lock: free }
		]);
	});

	it('keeps explicit groups before groups inferred from entries, including empty groups', () => {
		const doc: Doc = {
			...seedDoc(),
			variables: {
				'mixer.gain': { value: 1 },
				'patch.subject': { value: 'P07' },
				'mixer.pan': { value: 0, lock: { value: true } }
			},
			variable_groups: { system: { lock: { config: true } }, mixer: { lock: { config: true } } }
		};
		const views = variableViews(doc);
		const locks = variableGroupLocks(doc);
		const groups = groupedVariables(views, locks);
		expect(groups.map((g) => [g.group, g.entries.map((e) => e.element), g.lock])).toEqual([
			['system', [], { config: true, value: false }],
			['mixer', ['gain', 'pan'], { config: true, value: false }],
			['patch', ['subject'], { config: false, value: false }]
		]);
		// What holds an entry is its own lock and its group's together.
		expect(effectiveLock(views[2], locks.mixer)).toEqual({ config: true, value: true });
		expect(effectiveLock(views[1], locks.patch)).toEqual({ config: false, value: false });
	});

	it('holds a node name to the letters-and-digits rule', () => {
		expect(isValidName('ab2')).toBe(true);
		expect(isValidName('a_b')).toBe(false);
		expect(isValidName('_x')).toBe(false);
		expect(isValidName('1x')).toBe(false);
		expect(isValidName('class')).toBe(false);
	});

	it('validates names like the Rust identifier rule', () => {
		expect(isValidIdentifier('default_ufreq')).toBe(true);
		expect(isValidIdentifier('_x1')).toBe(true);
		expect(isValidIdentifier('')).toBe(false);
		expect(isValidIdentifier('1x')).toBe(false);
		expect(isValidIdentifier('a b')).toBe(false);
		expect(isValidIdentifier('a.b')).toBe(false);
		// A keyword passes the character rule and fails the parser, and every namespace is read as
		// an ATTRIBUTE in an expression — `variables.gain`, `nd('chain').drain` — so all refuse one.
		expect(isValidIdentifier('drain')).toBe(true);
		expect(isValidIdentifier('class')).toBe(false);
		expect(isValidIdentifier('None')).toBe(false);
		expect(isValidIdentifier('nd()')).toBe(false);
		expect(isValidIdentifier("it's")).toBe(false);
		expect(isValidIdentifier('lambda')).toBe(false);
		// `variables` is goofi's own namespace token, reserved for variables AND node names alike.
		expect(isValidIdentifier('variables')).toBe(false);
	});

	/* The arrangement parser. It reads a tree straight into the shape the panel system draws, so it
	   is the one place a malformed document could put a hole on screen — and it is fed by the wire,
	   which means anything it cannot make sense of has to be DROPPED rather than half-drawn. */
	it('reads the tab strip into the tree the panel system draws', () => {
		const doc = {
			arrangement: {
				'#seq': 4,
				tabs: [
					{
						id: 'tab-1',
						name: 'Tab 1',
						root: {
							kind: 'split',
							id: 'split-4',
							axis: 'column',
							children: [
								{ kind: 'panel', id: 'panel-2', size: 0.6, panel_type: 'node-editor', state: null },
								{
									kind: 'panel',
									id: 'panel-3',
									size: 0.4,
									panel_type: 'viewer',
									state: { node: 'a1b2', kind: 'line' }
								}
							]
						}
					}
				]
			}
		};
		expect(arrangementTabs(doc)).toEqual([
			{
				id: 'tab-1',
				name: 'Tab 1',
				root: {
					kind: 'split',
					id: 'split-4',
					direction: 'column',
					// A child's share rides ON the child, and the renderer wants them per split — so
					// they are collected on the way up rather than held twice.
					sizes: [0.6, 0.4],
					children: [
						{ kind: 'panel', id: 'panel-2', panelType: 'node-editor', state: undefined },
						{ kind: 'panel', id: 'panel-3', panelType: 'viewer', state: { node: 'a1b2', kind: 'line' } }
					]
				}
			}
		]);
	});

	it('drops what it cannot draw instead of drawing a hole', () => {
		// A tab whose root will not parse, a split with no children and a node with no id are each
		// a shape the manager never writes, and each would otherwise reach the renderer as a gap;
		// a panel's state is whatever bag it holds.
		const tab = (root: unknown): unknown => ({ id: 't', name: 'T', root });
		expect(arrangementTabs({ arrangement: {} })).toEqual([]);
		expect(arrangementTabs({ arrangement: { tabs: 'nope' } })).toEqual([]);
		expect(arrangementTabs({ arrangement: { tabs: [tab({ kind: 'panel' })] } })).toEqual([]);
		expect(
			arrangementTabs({ arrangement: { tabs: [tab({ kind: 'split', id: 's', children: [] })] } })
		).toEqual([]);
		expect(
			arrangementTabs({ arrangement: { tabs: [tab({ kind: 'panel', id: 'p', state: { node: 'x' } })] } })
		).toEqual([
			{ id: 't', name: 'T', root: { kind: 'panel', id: 'p', panelType: 'empty', state: { node: 'x' } } }
		]);
	});

	it('gives a root the whole tab, whatever the wire says about its share', () => {
		// A root carries no share on the wire — it fills its tab — so the parser must not read one.
		const [ws] = arrangementTabs({
			arrangement: { tabs: [{ id: 't', name: 'T', root: { kind: 'panel', id: 'p', panel_type: 'console' } }] }
		});
		expect(ws.root).toEqual({ kind: 'panel', id: 'p', panelType: 'console', state: undefined });
	});
});
