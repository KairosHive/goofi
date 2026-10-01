/** What the expression completion source knows about the patch: node names, output slots, variables. */
import { graph, type GraphStore } from '$lib/stores/graph.svelte';
import type { VariableView } from '$lib/crdt/graphDoc';

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
	/** Output slots; the LENGTH is load-bearing — a multi-output node raises unless a slot is named. */
	slots: CatalogueSlot[];
	/** Param groups, what `.params.<group>.<param>` reads. Empty on a port or a facade. */
	params: CatalogueGroup[];
}

export type CatalogueVariable = Pick<VariableView, 'name' | 'group' | 'element' | 'type'>;

export interface ExprCatalogue {
	nodes: CatalogueNode[];
	variables: CatalogueVariable[];
	/** The editing node's own display name — what `me` reads. */
	self?: string;
}

/** The live patch, read at the moment a completion is asked for. `g` is the store to read, so a
 * test can drive this against a seeded one. */
export function liveCatalogue(g: GraphStore = graph()): ExprCatalogue {
	return {
		// Everything `nd()` can name, facades included. A facade keys its slots by port uid and
		// `nd()` takes the port's name, so the label is offered.
		nodes: g.nodes.map((n) => ({
			name: n.name,
			slots: Object.entries(n.output_slots).map(([key, dtype]) => ({
				name: n.slot_labels?.[key] ?? key,
				dtype
			})),
			params: Object.entries(n.params ?? {}).map(([group, names]) => ({
				group,
				names: Object.keys(names)
			}))
		})),
		variables: g.variables
	};
}
