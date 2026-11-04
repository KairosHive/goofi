/** What the expression completion source knows about the patch: node names, output slots,
 *  variables, and the state machines' playheads and states. */
import { ME_FIELDS, PLAYHEAD_FIELDS } from '$lib/api/vocab';
import { graph, type GraphStore } from '$lib/stores/graph.svelte';
import type { VariableView } from '$lib/crdt/graphDoc';
import { engineOf } from '$lib/editor/typeId';
import { formOf, variableFrame, type Form } from '$lib/stores/variableValues.svelte';

export interface CatalogueSlot {
	name: string;
	dtype: string;
}

export interface CatalogueGroup {
	group: string;
	names: string[];
}

export interface CatalogueNode {
	/** The DISPLAY name, which is what `nd()` takes. */
	name: string;
	/** The engine the node runs on; null for a port. */
	engine: string | null;
	/** Output slots; the LENGTH is load-bearing — a multi-output node raises unless a slot is named. */
	slots: CatalogueSlot[];
	/** Param groups, what `.params.<group>.<param>` reads. Empty on a port or a facade. */
	params: CatalogueGroup[];
}

/** A variable, with the form its live value holds when a watcher has it; the document says nothing. */
export type CatalogueVariable = Pick<VariableView, 'name' | 'group' | 'element'> & { form?: Form };

/** A playhead as a param's `sm('name')` reads it: its machine, and the elements its group holds. */
export interface CataloguePlayhead {
	name: string;
	machine: string;
	/** The machine's own four, then one per attribute. */
	fields: string[];
}

/** A machine's own scope: what `sm('…')` may name, and what `me`, the playhead, answers. */
export interface CatalogueMachine {
	states: string[];
	playheads: string[];
	/** `me`'s fields, and a playhead's under `sm()`: the playhead's own, then one per attribute. */
	fields: string[];
}

export interface ExprCatalogue {
	nodes: CatalogueNode[];
	variables: CatalogueVariable[];
	playheads: CataloguePlayhead[];
	/** The editing node's own display name — what `me` reads. */
	self?: string;
	/** Set for a machine's expression, where `me` is the playhead and `sm()` a state or a playhead. */
	machine?: CatalogueMachine;
}

/** The scope of `machine`'s expressions, read at the moment a completion is asked for. */
export function machineCatalogue(machine: string, g: GraphStore = graph()): CatalogueMachine {
	const m = g.machines[machine];
	return {
		states: Object.keys(m?.states ?? {}),
		playheads: Object.keys(m?.playheads ?? {}),
		fields: [...ME_FIELDS, ...Object.keys(m?.attributes ?? {})]
	};
}

/** The live patch, read at the moment a completion is asked for. `g` is the store to read, so a
 * test can drive this against a seeded one. */
export function liveCatalogue(g: GraphStore = graph()): ExprCatalogue {
	return {
		// Everything `nd()` can name, facades included. A facade keys its slots by port uid and
		// `nd()` takes the port's name, so the label is offered.
		nodes: g.nodes.map((n) => ({
			name: n.name,
			engine: engineOf(n.type),
			slots: Object.entries(n.output_slots).map(([key, dtype]) => ({
				name: n.slot_labels?.[key] ?? key,
				dtype
			})),
			params: Object.entries(n.params ?? {}).map(([group, names]) => ({
				group,
				names: Object.keys(names)
			}))
		})),
		variables: g.variables.map((v) => {
			const frame = variableFrame(v.name);
			return { name: v.name, group: v.group, element: v.element, ...(frame ? { form: formOf(frame) } : {}) };
		}),
		playheads: Object.entries(g.machineGroups).map(([name, machine]) => ({
			name,
			machine,
			fields: [...PLAYHEAD_FIELDS, ...Object.keys(g.machines[machine]?.attributes ?? {})]
		}))
	};
}
