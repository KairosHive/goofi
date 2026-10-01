/** goofi's `LayoutHost` — the one place a layout gesture becomes a manager op plus its undo step. */
import type { LayoutHost, TabRef } from 'panelty';
import type { Direction } from 'panelty';
import { history } from './history.svelte';
import { getControl, type Control, type Step } from '$lib/api/control';
import type { OpName } from '$lib/api/ops';

/** What a placement answers: the entry it placed, and the tab it landed on. */
interface Placed {
	id: string;
	tab: string;
}

/** How the host reaches the manager. */
export interface HostDeps {
	control: () => Control;
}

/** Which SIDE of a target a drop lands on — the manager's spelling of the axis-and-half the panel
 * system raises as a pair. One word, so the two cannot disagree on the way over. */
const SIDE = { row: ['right', 'left'], column: ['bottom', 'top'] } as const;
const side = (d: Direction, placeBefore: boolean): string => SIDE[d][placeBefore ? 1 : 0];

export function goofiLayoutHost(deps: HostDeps): LayoutHost {
	/** Send one op; the manager records its step, and a refusal answers null. */
	async function cmd<T>(op: OpName, payload: Record<string, unknown>, step?: Step): Promise<T | null> {
		try {
			return await deps.control().call<T>(op, payload, step);
		} catch (e) {
			console.warn(`${op} refused`, e);
			return null;
		}
	}

	const landed = (v: unknown): boolean => v !== null;

	return {
		// The NAME is the manager's: it sees the whole strip under the lock the add runs on, so
		// nothing here has to reserve one against a replica that lands a round trip later.
		async addTab(opts): Promise<TabRef | null> {
			// Grouped so a tab that arrives already showing its panel type is one ctrl-Z.
			return await history().transaction('Add tab', async (step) => {
				const born = await cmd<Placed>('layout panel add', { index: opts?.index }, step);
				if (born && opts?.panelType) {
					await cmd('layout panel edit', { panel: born.id, type: opts.panelType }, step);
				}
				return born && { tab: born.tab, panel: born.id };
			});
		},

		async removeTab(tab) {
			return landed(await cmd('layout remove', { entry: tab }));
		},

		async renameTab(tab, name) {
			return landed(await cmd('layout tab edit', { tab, name }));
		},

		async reorderTab(tab, toIndex) {
			return landed(await cmd('layout move', { entry: tab, index: toIndex }));
		},

		// `beside`: the fresh panel is what is placed, and it lands beside this one.
		async splitPanel(panel, direction: Direction, placeBefore, ratio) {
			const fresh = await cmd<Placed>('layout panel add', {
				beside: panel,
				side: side(direction, placeBefore),
				ratio
			});
			return fresh?.id ?? null;
		},

		async removePanel(panel) {
			return landed(await cmd('layout remove', { entry: panel }));
		},

		async resizeSplit(split, fractions, preview = false) {
			const payload = { split, fraction: fractions };
			if (!preview) return landed(await cmd('layout split edit', payload));
			deps.control().preview(`split ${split}`, 'layout split edit', payload);
			return true;
		},

		async setPanel(panel, patch, label = 'Change panel') {
			return landed(await cmd('layout panel edit', { panel, ...patch }));
		},

		// One op either way: a drop onto the tab bar names no target, a drop on an edge names one.
		async movePanel(subtree, to) {
			if ('newTab' in to) {
				return landed(
					await cmd('layout move', { entry: subtree, index: to.newTab })
				);
			}
			return landed(
				await cmd('layout move', {
					entry: subtree,
					beside: to.panel,
					side: side(to.direction, to.placeBefore)
				})
			);
		}
	};
}

/** The live host, wired to the socket. */
let _live: LayoutHost | null = null;
export function layoutHost(): LayoutHost {
	if (!_live) _live = goofiLayoutHost({ control: getControl });
	return _live;
}
