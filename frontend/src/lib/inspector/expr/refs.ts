/** What the reference picker offers: the nodes with an output a param may reference, and those
 *  outputs — the typing rule the manager holds, applied before the pick. */
import type { CatalogueSlot, ExprCatalogue } from './catalogue';
import type { ComboOption } from '$lib/ui';
import { feeds, type SlotDtype } from '$lib/api/vocab';

/** The output kind a param of `type` may reference: a string reads a STRING, everything else an ARRAY. */
export function wantedDtype(paramType: string): SlotDtype {
	return paramType === 'string' ? 'STRING' : 'ARRAY';
}

const fitting = (slots: CatalogueSlot[], dtype: SlotDtype) => slots.filter((s) => feeds(s.dtype as SlotDtype, dtype));

export function refNodes(cat: ExprCatalogue, dtype: SlotDtype): ComboOption[] {
	return cat.nodes.flatMap((n) => {
		const slots = fitting(n.slots, dtype);
		return slots.length ? [{ label: n.name, detail: slots.map((s) => s.name).join(', ') }] : [];
	});
}

export function refSlots(cat: ExprCatalogue, node: string, dtype: SlotDtype): ComboOption[] {
	return fitting(cat.nodes.find((n) => n.name === node)?.slots ?? [], dtype).map((s) => ({ label: s.name, detail: s.dtype }));
}

/** `node.slot` split at its one dot; a malformed or empty value is two empty halves. */
export function splitReference(reference: string | null): [string, string] {
	const at = reference?.indexOf('.') ?? -1;
	if (!reference || at < 0) return ['', ''];
	return [reference.slice(0, at), reference.slice(at + 1)];
}
