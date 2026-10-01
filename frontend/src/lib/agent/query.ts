/** Agent read/introspection surface — paired with `commands`. */
import { graph } from '$lib/stores/graph.svelte';
import { selection } from '$lib/stores/selection.svelte';
import { workspace } from 'panelty';
import { history } from '$lib/stores/history.svelte';
import { arrivalRate, latestHead } from '$lib/api/frames';
import type { FrameHead } from '$lib/api/dataProtocol';
import { collectPanels } from 'panelty';
import { asStateObject, linkedNodeName } from 'panelty';
import { reconstructMeta } from '$lib/editor/metaFormat';

import type { LinkInfo, NodeInstanceInfo, NodeTypeInfo } from '$lib/api/control';
import type { VariableView } from '$lib/crdt/graphDoc';

export interface FrameSummary {
	dtype: string;
	shape?: number[];
	/** Element count of the reduced wire array `numeric` covers; `shape` stays the node's TRUE original. */
	reducedLength?: number;
	numeric?: { min: number; max: number; mean: number };
	text?: string;
	/** The producer's emit counter (`meta.index`): a greater one is a newer frame. */
	index?: number;
}

const shapesEqual = (a: number[], b: readonly number[]): boolean =>
	a.length === b.length && a.every((n, i) => n === b[i]);

/** A compact, DOM-free description of the latest frame on a slot, from what its head says:
 * the same words whether this thread holds the frame or the worker draws it. */
function summarize(head: FrameHead | null): FrameSummary | null {
	if (!head) return null;
	const index = typeof head.meta.index === 'number' ? head.meta.index : undefined;
	if (head.array) {
		const a = head.array;
		const recon = reconstructMeta(head.meta);
		const shape = Array.isArray(recon.shape) ? (recon.shape as number[]) : a.wireShape;
		// An AXIS reduction, so a frame whose only `reduced` entry is the depth is not one.
		const reduced = !shapesEqual(shape, a.wireShape);
		return {
			dtype: a.dtype,
			shape,
			numeric: a.min !== null ? { min: a.min, max: a.max as number, mean: a.mean as number } : undefined,
			...(reduced ? { reducedLength: a.length } : {}),
			index
		};
	}
	if (head.dtype === 'STRING') return { dtype: 'STRING', text: head.text, index };
	return { dtype: head.dtype, index };
}

export interface PanelView {
	panelId: string;
	type: string;
	node: string | null;
	slot: string | null;
	kind: string | null;
}

export const query = {
	graph: (): {
		nodes: NodeInstanceInfo[];
		links: LinkInfo[];
		savePath: string | null;
		unsavedChanges: boolean;
	} => {
		const g = graph();
		return {
			nodes: g.nodes,
			links: g.links,
			savePath: g.savePath,
			unsavedChanges: g.unsavedChanges
		};
	},
	/** Every armed output slot, as the document holds it. */
	armed: (): { uid: string; slot: string }[] => graph().armed,
	nodeTypes: (): NodeTypeInfo[] | null => graph().nodeTypes,
	/** Whether the replica has pulled from the manager yet; until true, `graph()` reads describe an EMPTY replica. */
	docSynced: (): boolean => graph().docSynced,
	/** Every patch variable (system + user), in system-first/creation order. */
	variables: (): VariableView[] => graph().variables,
	node: (uid: string): NodeInstanceInfo | null => graph().nodeById(uid),
	nodeParams: (uid: string): NodeInstanceInfo['params'] | null =>
		graph().nodeById(uid)?.params ?? null,
	selection: (
		panelId: string | null = workspace().activePanelId
	): { nodes: string[]; edges: string[] } => {
		const sel = selection();
		return { nodes: [...sel.nodes(panelId)], edges: [...sel.edges(panelId)] };
	},
	frameSummary: (node: string, slot: string): FrameSummary | null =>
		summarize(latestHead(node, slot)),
	/** Frames a second the WIRE delivered for one stream — what a paint count cannot show. */
	arrivalRate: (node: string, slot: string): number | null => arrivalRate(node, slot),
	panels: (): PanelView[] =>
		collectPanels(workspace().active.root).map((p) => {
			const s = asStateObject(p.state);
			return {
				panelId: p.id,
				type: p.panelType,
				node: linkedNodeName(p.state),
				slot: typeof s.slot === 'string' ? s.slot : null,
				kind: typeof s.kind === 'string' ? s.kind : null
			};
		}),

	canUndo: (): boolean => history().canUndo,
	canRedo: (): boolean => history().canRedo,
	undoLabel: (): string | null => history().undoLabel
};

export type Query = typeof query;
