/* `use:dropTarget`: the element takes what the engine pairs with its tags. Null unregisters it. */
import type { ActionReturn } from 'svelte/action';
import { drag, type DropTarget } from './engine.svelte';

export type DropTargetOptions = Omit<DropTarget, 'el'>;

export function dropTarget(el: HTMLElement, opts: DropTargetOptions | null): ActionReturn<DropTargetOptions | null> {
	const engine = drag();
	let record: DropTarget | null = null;
	let off: (() => void) | null = null;
	// One record for the element's life, updated in place: a re-render must not drop the target
	// the engine holds as `over` mid-drag.
	const apply = (next: DropTargetOptions | null): void => {
		if (!next) {
			off?.();
			off = null;
			record = null;
		} else if (record) {
			record.id = next.id;
			record.tags = next.tags;
			record.accept = next.accept;
			record.onDrop = next.onDrop;
		} else {
			record = { el, ...next };
			off = engine.register(record);
		}
	};
	apply(opts);
	return {
		update: apply,
		destroy: () => off?.()
	};
}
