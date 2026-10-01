/** Post-undo/redo highlight pulse over the affected nodes. */
import { flash } from './flash.svelte';
import type { NavContext } from './navContext';
import type { GraphStore } from './graph.svelte';

export function pulseRestored(ctx: NavContext, g: GraphStore): void {
	const names = new Set<string>();
	for (const s of Object.values(ctx.selection)) for (const n of s.nodes) names.add(n);
	if (!names.size) return;

	// A replay arrives as a whole-doc re-mirror, so a re-created node has no echo to await.
	for (const name of names) {
		if (g.nodeById(name)) flash().pulse([name]);
	}
}
