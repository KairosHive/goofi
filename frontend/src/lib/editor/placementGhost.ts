/** The node a placement shows before it is born: a record of the type, drawn by the same card as
 *  every live node, so the two can never diverge. Its viewers open collapsed, since it has no feed. */
import type { NodeInstanceInfo, NodeTypeInfo } from '$lib/api/control';
import { bareName } from './typeId';

export const GHOST_UID = '__ghost__';

export function ghostNode(t: NodeTypeInfo, scope: string): NodeInstanceInfo {
	return {
		uid: GHOST_UID,
		name: bareName(t.type),
		type: t.type,
		doc: t.doc,
		editor: t.editor,
		input_slots: t.input_slots,
		input_multi: t.input_multi,
		output_slots: t.output_slots,
		params: {},
		pos: [0, 0],
		viewers: Object.fromEntries(Object.keys(t.output_slots).map((slot) => [slot, { collapsed: true }])),
		scope,
		error: null
	};
}
