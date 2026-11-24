/** The one quick pick open at a time: a drop or a gesture asks, the shell draws the list, and the
 * asker gets the picked id, or null when the list was let go. */
import type { PickOption } from '$lib/ui/QuickPick.svelte';

export interface QuickPickRequest {
	at: { x: number; y: number };
	options: PickOption[];
	label: string;
	resolve: (id: string | null) => void;
}

export class QuickPickStore {
	pending = $state.raw<QuickPickRequest | null>(null);

	/** Ask for one of `options` at `at`; a request already open is let go unpicked. */
	ask(at: { x: number; y: number }, options: PickOption[], label: string): Promise<string | null> {
		this.settle(null);
		return new Promise((resolve) => {
			this.pending = { at, options, label, resolve };
		});
	}

	/** Answer the open request, if any. */
	settle(id: string | null): void {
		const p = this.pending;
		if (!p) return;
		this.pending = null;
		p.resolve(id);
	}
}

let _picks: QuickPickStore | null = null;
export function quickPick(): QuickPickStore {
	if (!_picks) _picks = new QuickPickStore();
	return _picks;
}
