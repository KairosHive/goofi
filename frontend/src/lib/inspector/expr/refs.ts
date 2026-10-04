/** What the reference picker offers: the nodes with an output a param may reference, and those
 *  outputs — the typing rule the manager holds, applied before the pick. */
import type { CatalogueNode, CatalogueSlot, ExprCatalogue } from './catalogue';
import type { ComboOption } from '$lib/ui';
import { ENGINE_LOCAL, feeds, type SlotDtype } from '$lib/api/vocab';

/** The output kind a param of `type` may reference: a string reads a STRING, everything else an ARRAY. */
export function wantedDtype(paramType: string): SlotDtype {
	return paramType === 'string' ? 'STRING' : 'ARRAY';
}

/** The producer's outputs the reader's param may take: its own kind, or the producer's engine-local
 *  kind when both nodes share that plane — a plan edge, which is how audio reads audio. */
function fitting(cat: ExprCatalogue, producer: CatalogueNode, dtype: SlotDtype, reader?: string): CatalogueSlot[] {
	const plane = reader && producer.engine && cat.nodes.find((n) => n.name === reader)?.engine === producer.engine;
	return producer.slots.filter(
		(s) => feeds(s.dtype as SlotDtype, dtype) || (plane && dtype === 'ARRAY' && ENGINE_LOCAL.has(s.dtype as SlotDtype))
	);
}

export function refNodes(cat: ExprCatalogue, dtype: SlotDtype, reader?: string): ComboOption[] {
	return cat.nodes.flatMap((n) => {
		const slots = fitting(cat, n, dtype, reader);
		return slots.length ? [{ label: n.name, detail: slots.map((s) => s.name).join(', ') }] : [];
	});
}

export function refSlots(cat: ExprCatalogue, node: string, dtype: SlotDtype, reader?: string): ComboOption[] {
	const producer = cat.nodes.find((n) => n.name === node);
	return producer ? fitting(cat, producer, dtype, reader).map((s) => ({ label: s.name, detail: s.dtype })) : [];
}

/** `node.slot` split at its one dot; a malformed or empty value is two empty halves. */
export function splitReference(reference: string | null): [string, string] {
	const at = reference?.indexOf('.') ?? -1;
	if (!reference || at < 0) return ['', ''];
	return [reference.slice(0, at), reference.slice(at + 1)];
}
