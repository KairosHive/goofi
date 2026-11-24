/** Per-(node, slot) INLINE viewer view-state, derived on read from the node's `viewers` blob. */
import type { NodeInstanceInfo } from '$lib/api/control';
import type { ViewerKind } from './registry';
import type { SettingsMap } from './module';

export interface SlotView {
	collapsed?: boolean;
	kind?: ViewerKind;
	settings?: SettingsMap;
}

/** A slot's stored view, RAW — no dtype resolution, no setting defaults. Empty when unset. */
export function slotView(node: NodeInstanceInfo | null | undefined, slot: string): SlotView {
	return (node?.viewers?.[slot] as SlotView | undefined) ?? {};
}

/** Is a slot's inline viewer open? Default visible, but collapsed on a node with 3+ outputs. */
export function isSlotExpanded(node: NodeInstanceInfo | null | undefined, slot: string): boolean {
	return !(slotView(node, slot).collapsed ?? Object.keys(node?.output_slots ?? {}).length >= 3);
}
