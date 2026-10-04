/**
 * The browser replica of goofi's control-plane document, as `goofi_bridge::projection` builds it.
 * Every reader is total: an absent or wrongly-typed leaf answers a default rather than throwing.
 */
import { EMPTY_PANEL_TYPE, SCOPE_TYPE, boundaryType, type ControlKindId } from '$lib/api/vocab';
import { VIDEO_QUALITIES, type VideoQuality } from '$lib/api/types';
import type { Link, Literal, Lock, Midi, VariableSource } from '$lib/api/generated';
import type { LayoutNode, Workspace } from 'panelty';
import { obj, type Obj } from './ops';

export type Doc = Record<string, unknown>;

export function emptyDoc(): Doc {
	return { nodes: {}, links: {}, variables: {}, arrangement: {} };
}

/** What a sub-patch facade exposes, derived from the records that name it. */
export interface FacadeFace {
	input_slots: Record<string, string>;
	output_slots: Record<string, string>;
	slot_labels: Record<string, string>;
	memberCount: number;
}

const str = (m: Obj | undefined, key: string): string => {
	const v = m?.[key];
	return typeof v === 'string' ? v : '';
};
const optStr = (m: Obj | undefined, key: string): string | undefined => {
	const v = m?.[key];
	return typeof v === 'string' ? v : undefined;
};

export function nodesMap(doc: Doc): Record<string, Obj> {
	return obj(doc.nodes) as Record<string, Obj>;
}

/** Each facade's face, keyed by its uid: a PORT is the facade's slot, keyed by the port's stable
 * uid and labelled with its renameable name, so a rename relabels without re-keying the wire. */
export function facadeFaces(doc: Doc): Map<string, FacadeFace> {
	const out = new Map<string, FacadeFace>();
	const at = (uid: string): FacadeFace => {
		let f = out.get(uid);
		if (!f) out.set(uid, (f = { input_slots: {}, output_slots: {}, slot_labels: {}, memberCount: 0 }));
		return f;
	};
	for (const [uid, rec] of Object.entries(nodesMap(doc))) {
		if (str(rec, 'type') === SCOPE_TYPE) at(uid);
		const parent = optStr(rec, 'scope');
		if (!parent) continue;
		const face = at(parent);
		face.memberCount++;
		const bnd = boundaryType(str(rec, 'type'));
		if (!bnd) continue;
		(bnd.dir === 'in' ? face.input_slots : face.output_slots)[uid] = bnd.dtype;
		face.slot_labels[uid] = str(rec, 'name');
	}
	return out;
}

/** Every armed output slot, node by node, in the order the document holds them. */
export function recordedSlots(doc: Doc): { uid: string; slot: string; quality: VideoQuality }[] {
	const out: { uid: string; slot: string; quality: VideoQuality }[] = [];
	for (const [uid, n] of Object.entries(nodesMap(doc))) {
		const r = n?.record;
		if (!Array.isArray(r)) continue;
		for (const entry of r) {
			const { slot, quality } = obj(entry);
			if (typeof slot === 'string' && VIDEO_QUALITIES.includes(quality as VideoQuality)) {
				out.push({ uid, slot, quality: quality as VideoQuality });
			}
		}
	}
	return out;
}

/** The links in connection order — a keyed map, so a wire made or cut is one key's delta. */
export function linkViews(doc: Doc): Link[] {
	return (Object.values(obj(doc.links)) as Obj[]).map((m) => ({
		node_out: str(m, 'node_out'),
		slot_out: str(m, 'slot_out'),
		node_in: str(m, 'node_in'),
		slot_in: str(m, 'slot_in')
	}));
}

/** A control element's widget, its range and its place in the panel's grid. */
export interface ControlView {
	/** The generated union, never a restatement of it: a kind added in `vocab.rs` reaches this
	    view without a second list to remember. */
	kind: ControlKindId;
	min?: number;
	max?: number;
	step?: number;
	options?: string[];
	x: number;
	y: number;
	w: number;
	h: number;
}

/** What holds a variable or a group: `config` its name, widget and membership, `value` its value. */
export type LockView = Lock;

/** What a variable follows; `error` says why it delivers nothing (its node or output is gone). */
export type SourceView = VariableSource & { error?: string };

export interface VariableView {
	/** The full `group.element` — what an expression spells and every op names. */
	name: string;
	group: string;
	element: string;
	/** Present when this variable is a control-panel element. */
	control?: ControlView;
	/** Present when the manager writes this variable from a producer; nobody else may set it. */
	source?: SourceView;
	/** The variable's OWN lock; its group's reaches it too — see `effectiveLock`. */
	lock: LockView;
}

function sourceOf(raw: unknown): SourceView | undefined {
	const s = obj(raw);
	if (typeof s.reference !== 'string') return undefined;
	const view: SourceView = { reference: s.reference };
	if (typeof s.index === 'number') view.index = s.index;
	if (typeof s.error === 'string') view.error = s.error;
	return view;
}

function lockOf(raw: unknown): LockView {
	const l = obj(raw);
	return { config: l.config === true, value: l.value === true };
}

/** Explicit groups and their built-in flags, by name: a device's group is locked whole. */
export function variableGroupLocks(doc: Doc): Record<string, LockView> {
	const out: Record<string, LockView> = {};
	for (const [group, rec] of Object.entries(obj(doc.variable_groups))) {
		out[group] = obj(rec).midi ? { config: true, value: true } : lockOf(obj(rec).lock);
	}
	return out;
}

/** Every group that reads a MIDI device, by name: the bus MIDI learn listens to. */
export function midiGroups(doc: Doc): Record<string, Midi> {
	const out: Record<string, Midi> = {};
	for (const [group, rec] of Object.entries(obj(doc.variable_groups))) {
		const midi = obj(rec).midi;
		if (typeof midi === 'object' && midi && typeof (midi as Midi).port === 'string') out[group] = midi as Midi;
	}
	return out;
}

/** What holds `gv` right now: its own lock and its group's together. */
export function effectiveLock(gv: VariableView, group: LockView | undefined): LockView {
	return { config: gv.lock.config || group?.config === true, value: gv.lock.value || group?.value === true };
}

/** All variables, in the document's key order (system-first, then user in creation order). */
export function variableViews(doc: Doc): VariableView[] {
	const out: VariableView[] = [];
	for (const [name, raw] of Object.entries(obj(doc.variables))) {
		const g = obj(raw);
		const dot = name.indexOf('.');
		if (dot > 0) {
			out.push({
				name,
				group: name.slice(0, dot),
				element: name.slice(dot + 1),
				control: (g.control as ControlView | undefined) ?? undefined,
				source: sourceOf(g.source),
				lock: lockOf(g.lock)
			});
		}
	}
	return out;
}

export interface VariableGroupView {
	group: string;
	entries: VariableView[];
	lock: LockView;
}

/** Explicit groups keep their order even when empty; groups inferred from entries follow them. */
export function groupedVariables(views: VariableView[], locks: Record<string, LockView> = {}): VariableGroupView[] {
	const out: VariableGroupView[] = Object.entries(locks).map(([group, lock]) => ({ group, entries: [], lock }));
	for (const entry of views) {
		const group = out.find((group) => group.group === entry.group);
		if (group) group.entries.push(entry);
		else out.push({ group: entry.group, entries: [entry], lock: { config: false, value: false } });
	}
	return out;
}

/** Parse one layout node, answering the share it takes of its parent — a split carries its children's shares. */
function layoutNode(raw: unknown, root: boolean): { node: LayoutNode; size: number } | null {
	const n = obj(raw);
	const id = optStr(n, 'id');
	if (!id) return null;
	// A root fills its tab and carries no share on the wire.
	const size = root ? 1 : typeof n.size === 'number' ? n.size : 0;
	if (n.kind === 'panel') {
		return {
			node: { kind: 'panel', id, panelType: optStr(n, 'panel_type') ?? EMPTY_PANEL_TYPE, state: n.state ?? undefined },
			size
		};
	}
	if (n.kind !== 'split' || !Array.isArray(n.children)) return null;
	const children: LayoutNode[] = [];
	const sizes: number[] = [];
	for (const c of n.children) {
		const parsed = layoutNode(c, false);
		if (!parsed) continue;
		children.push(parsed.node);
		sizes.push(parsed.size);
	}
	if (children.length === 0) return null;
	return {
		node: { kind: 'split', id, direction: n.axis === 'column' ? 'column' : 'row', children, sizes },
		size
	};
}

/** The tab strip as the panel system draws it; a tab whose root will not parse is dropped. */
export function arrangementTabs(doc: Doc): Workspace[] {
	const raw = obj(doc.arrangement).tabs;
	if (!Array.isArray(raw)) return [];
	const out: Workspace[] = [];
	for (const t of raw) {
		const tab = obj(t);
		const id = optStr(tab, 'id');
		const parsed = layoutNode(tab.root, true);
		if (!id || !parsed) continue;
		out.push({ id, name: optStr(tab, 'name') ?? '', root: parsed.node });
	}
	return out;
}

/** Python's keywords, plus goofi's own namespace token `variables`: a regex reads each as an
 * identifier and a parser does not. */
const RESERVED = new Set(
	`variables False None True and as assert async await break class continue def del elif else
	 except finally for from global if import in is lambda nonlocal not or pass raise return try
	 while with yield`.split(/\s+/)
);

/** Whether `name` is legal in the ONE expression namespace, as the Rust `is_valid_identifier` says.
 * An expression reads every name as an ATTRIBUTE: `variables.gain`, `nd('chain').drain`. */
export function isValidIdentifier(name: string): boolean {
	return /^[A-Za-z_][A-Za-z0-9_]*$/.test(name) && !RESERVED.has(name);
}

/** The NODE name rule, the mirror of the Rust `is_valid_name`: a letter then letters or digits, not
 * a keyword — a reference spells `node.slot`, so no underscore either. Variables keep the rule above. */
export function isValidName(name: string): boolean {
	return /^[A-Za-z][A-Za-z0-9]*$/.test(name) && !RESERVED.has(name);
}
