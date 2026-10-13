/** An edit of several selected states or transitions at once, as ONE undo step, and what the
 * inspector shows of a field the selection may not share. */
import type { Literal, Machine } from '$lib/api/generated';
import { graph, type MachineOp } from '$lib/stores/graph.svelte';
import { history } from '$lib/stores/history.svelte';
import { notify } from '$lib/stores/notify.svelte';

/** `patch` into each of `ids` through `op`, the id under `key`. */
export async function editEach(machine: string, op: MachineOp, key: 'name' | 'id', ids: string[], patch: Record<string, unknown>): Promise<void> {
	const g = graph();
	try {
		await history().transaction(op, async (step) => {
			for (const id of ids) await g.machine(op, { machine, [key]: id, ...patch }, step);
		});
	} catch (e) {
		notify().failure(op, e);
	}
}

/** Whether `values` hold more than one value. */
export const mixed = (values: unknown[]): boolean => new Set(values.map((v) => JSON.stringify(v))).size > 1;

/** What the states hold for `attr`: the value shown (the first set, else the default), whether
 * any sets it, and whether they differ, unset counting as a value of its own. */
export function held(m: Machine, ids: string[], attr: string): { value: Literal; set: boolean; mixed: boolean } {
	const values = ids.map((id) => m.states[id]?.values?.[attr]);
	const first = values.find((v) => v !== undefined);
	return { value: first ?? m.attributes[attr].default, set: first !== undefined, mixed: mixed(values) };
}
