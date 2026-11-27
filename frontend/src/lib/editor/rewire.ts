/** The cable gestures as arithmetic on links: what a drop onto a cable, a swap or an unplug
 *  removes and adds. Pure, so the canvas only reads a plan and runs it. */
import type { LinkInfo, NodeTypeInfo } from '$lib/api/control';
import { feeds, type SlotDtype } from '$lib/api/vocab';

/** One end of a cable: a slot of a node, and the kind it carries. */
export interface End {
	node: string;
	slot: string;
	dtype: string;
}

/** A cable, from its output end to its input end. */
export interface Cable {
	from: End;
	to: End;
}

/** The node that carries one kind onto another plane, and the slots a cable takes through it. */
export interface Converter {
	type: string;
	input: string;
	output: string;
}

export type Edit =
	| { op: 'remove'; link: LinkInfo }
	/** `via` names the converter the wire passes through; null is a bare cable. */
	| { op: 'add'; link: LinkInfo; via: Converter | null };

/** The plane a kind lives on: the engine that takes it, and the name its seam node carries. */
const PLANE: Partial<Record<SlotDtype, { engine: string; name: string }>> = {
	ARRAY: { engine: 'signal', name: 'Signal' },
	AUDIO: { engine: 'audio', name: 'Audio' },
	TEXTURE: { engine: 'graphics', name: 'Graphics' }
};

/** The seam node from `from` to `to`: `<engine of to>:<plane of from>In`, when the library has it. */
export function converter(types: readonly NodeTypeInfo[], from: string, to: string): Converter | null {
	const src = PLANE[from as SlotDtype];
	const dst = PLANE[to as SlotDtype];
	if (!src || !dst || from === to) return null;
	const t = types.find((t) => t.type === `${dst.engine}:${src.name}In` && t.available);
	const input = Object.entries(t?.input_slots ?? {}).find(([, d]) => d === from)?.[0];
	const output = Object.entries(t?.output_slots ?? {}).find(([, d]) => d === to)?.[0];
	return t && input && output ? { type: t.type, input, output } : null;
}

/** How `from` reaches `to`: bare, through a converter, or not at all. */
export type Bridge = 'direct' | Converter | null;

export function bridge(types: readonly NodeTypeInfo[], from: string, to: string): Bridge {
	return feeds(from as SlotDtype, to as SlotDtype) ? 'direct' : converter(types, from, to);
}

export const linkOf = (c: Cable): LinkInfo => ({
	node_out: c.from.node,
	slot_out: c.from.slot,
	node_in: c.to.node,
	slot_in: c.to.slot
});

/** The wire `from` to `to`, with a converter where the kinds differ; null when none carries it. */
export function wireEdit(types: readonly NodeTypeInfo[], from: End, to: End): Edit | null {
	const b = bridge(types, from.dtype, to.dtype);
	if (!b) return null;
	return { op: 'add', link: linkOf({ from, to }), via: b === 'direct' ? null : b };
}

/** The wire `from` to `to` as a bare cable; null when the kinds differ. */
function addBare(from: End, to: End): Edit | null {
	return feeds(from.dtype as SlotDtype, to.dtype as SlotDtype) ? { op: 'add', link: linkOf({ from, to }), via: null } : null;
}

const all = (edits: (Edit | null)[]): Edit[] | null => (edits.every((e) => e !== null) ? (edits as Edit[]) : null);

/** Whether a plan places a converter: what the glow of its cable says. */
export const converts = (edits: readonly Edit[]): boolean => edits.some((e) => e.op === 'add' && e.via !== null);

/** A node put into a cable: the cable goes, and its two ends take the node's `input` and `output`. */
export function insertEdits(types: readonly NodeTypeInfo[], cable: Cable, input: End, output: End): Edit[] | null {
	return all([{ op: 'remove', link: linkOf(cable) }, wireEdit(types, cable.from, input), wireEdit(types, output, cable.to)]);
}

/** A cable dragged from `drag` onto `cable`: the new cable keeps the old one's far end. */
export function replaceEdits(types: readonly NodeTypeInfo[], cable: Cable, drag: End, side: 'source' | 'target'): Edit[] | null {
	return all([
		{ op: 'remove', link: linkOf(cable) },
		side === 'source' ? wireEdit(types, drag, cable.to) : wireEdit(types, cable.from, drag)
	]);
}

/** The cable with its `side` end put on `end`. */
const reseat = (c: Cable, side: 'source' | 'target', end: End): Cable => (side === 'source' ? { from: end, to: c.to } : { from: c.from, to: end });

const endOf = (c: Cable, side: 'source' | 'target'): End => (side === 'source' ? c.from : c.to);

/** Every cable of `cables` on the slot `at`, taken by its `side` end. */
export function cablesAt(cables: readonly Cable[], side: 'source' | 'target', at: { node: string; slot: string }): Cable[] {
	return cables.filter((c) => endOf(c, side).node === at.node && endOf(c, side).slot === at.slot);
}

/** Every cable on `side` of slot `a` moved to slot `b`: an unplug put down on another slot.
 *  Bare cables only; null when a kind differs. */
export function moveEdits(moved: readonly Cable[], side: 'source' | 'target', to: End): Edit[] | null {
	return all([
		...moved.map((c): Edit => ({ op: 'remove', link: linkOf(c) })),
		...moved.map((c) => {
			const next = reseat(c, side, to);
			return addBare(next.from, next.to);
		})
	]);
}

/** The cables of slots `a` and `b` of one node exchanged, an empty slot included. */
export function swapEdits(cables: readonly Cable[], side: 'source' | 'target', a: End, b: End): Edit[] | null {
	const atA = cablesAt(cables, side, a);
	const atB = cablesAt(cables, side, b);
	if (atA.length + atB.length === 0) return null;
	const reseated = [...atA.map((c) => reseat(c, side, b)), ...atB.map((c) => reseat(c, side, a))];
	return all([
		...[...atA, ...atB].map((c): Edit => ({ op: 'remove', link: linkOf(c) })),
		...reseated.map((c) => addBare(c.from, c.to))
	]);
}

/** A cable replaced by the cables an unplug holds: each keeps its fixed end and takes the far
 *  end of `cable` on the loose one. */
export function redirectEdits(types: readonly NodeTypeInfo[], moved: readonly Cable[], side: 'source' | 'target', cable: Cable): Edit[] | null {
	return all([
		...moved.map((c): Edit => ({ op: 'remove', link: linkOf(c) })),
		{ op: 'remove', link: linkOf(cable) },
		...moved.map((c) => (side === 'target' ? wireEdit(types, c.from, cable.to) : wireEdit(types, cable.from, c.to)))
	]);
}

/** One row of the slot popover. */
export interface SlotChoice extends End {
	/** No cable holds it; a list input is always free. */
	free: boolean;
	/** How the cable's end reaches it; null is a slot the cable cannot take. */
	bridge: Bridge;
}

/** The slots of one side of `node` as the popover lists them, against the cable end `peer`. */
export function slotChoices(
	types: readonly NodeTypeInfo[],
	node: { uid: string; input_slots: Record<string, string>; output_slots: Record<string, string>; input_multi?: string[] },
	side: 'input' | 'output',
	peer: End,
	cables: readonly Cable[]
): SlotChoice[] {
	const slots = side === 'input' ? node.input_slots : node.output_slots;
	return Object.entries(slots ?? {}).map(([slot, dtype]) => ({
		node: node.uid,
		slot,
		dtype,
		free:
			side === 'input'
				? (node.input_multi ?? []).includes(slot) || cablesAt(cables, 'target', { node: node.uid, slot }).length === 0
				: cablesAt(cables, 'source', { node: node.uid, slot }).length === 0,
		bridge: side === 'input' ? bridge(types, peer.dtype, dtype) : bridge(types, dtype, peer.dtype)
	}));
}

/** The slot a drop takes with no pick: the first free one the cable can reach, else the first. */
export function defaultInput(choices: readonly SlotChoice[]): SlotChoice | null {
	const able = choices.filter((c) => c.bridge !== null);
	return able.find((c) => c.free) ?? able[0] ?? null;
}

/** The output a drop takes with no pick: the first the cable can reach. An output fans out. */
export function defaultOutput(choices: readonly SlotChoice[]): SlotChoice | null {
	return choices.find((c) => c.bridge !== null) ?? null;
}
