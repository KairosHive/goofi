/** Graph selection, keyed per editor panel; a selection is replaced, never mutated in place. */
import { graph } from './graph.svelte';
import type { NodeInstanceInfo } from '$lib/api/control';
import { sameKeys } from '$lib/editor/slotProximity';
import type { MenuItem } from 'panelty';
import { copyText } from '$lib/clipboard';

/** What a param may take as its source: a control element, or one node's output slot. */
export type ReferencePick = { variable: string } | { node: string; slot: string };

interface PanelSel {
	nodes: Set<string>;
	edges: Set<string>;
}
const EMPTY: PanelSel = { nodes: new Set(), edges: new Set() };

function toggled(set: Set<string>, x: string): Set<string> {
	const next = new Set(set);
	if (!next.delete(x)) next.add(x);
	return next;
}

class SelectionStore {
	/** Per-editor-panel selection, keyed by panel id. */
	private map = $state<Record<string, PanelSel>>({});
	/** Last-focused editor panel — the standalone panels follow this one. */
	activeEditorId = $state<string | null>(null);
	/** Per-editor inspector visibility, keyed by panel id. Absent = enabled. */
	private inspectorOn = $state<Record<string, boolean>>({});
	/** Per-editor TRANSIENT dismissal (the ✕), cleared in `write()` — the one choke-point every
	 * real selection change funnels through. `inspectorOn` is the standing preference (the ◧). */
	private inspectorDismissed = $state<Record<string, boolean>>({});
	/** While on, a plain click adds to the selection — the coarse-pointer stand-in for
	 * shift/ctrl/meta. Session-wide, so `forgetAll` leaves it. */
	multiSelect = $state(false);
	/** The one app-wide pick a param's "reference selection" reads; null is none. */
	reference = $state<ReferencePick | null>(null);

	/** The two items a control element and an output slot both offer; `name` is what a copy gives. */
	referenceItems(pick: ReferencePick, name: string): MenuItem[] {
		return [
			{ label: 'Copy name', icon: 'copy', action: () => void copyText(name) },
			{ label: 'Select for reference', icon: 'circle-dot', action: () => (this.reference = pick) }
		];
	}

	toggleMultiSelect(): void {
		this.multiSelect = !this.multiSelect;
	}

	private sel(panelId: string | null): PanelSel {
		return (panelId && this.map[panelId]) || EMPTY;
	}
	private write(panelId: string, next: PanelSel): void {
		// A no-op write would allocate a fresh selection object, retriggering the editor's flowNodes
		// effect mid-drag so Svelte Flow's onnodedragstart never fires.
		const cur = this.sel(panelId);
		if (sameKeys(cur.nodes, next.nodes) && sameKeys(cur.edges, next.edges)) return;
		this.map = { ...this.map, [panelId]: next };
		// A real selection change re-arms a dismissed inspector.
		this.undismiss(panelId);
	}
	private undismiss(panelId: string): void {
		if (!this.inspectorDismissed[panelId]) return;
		const { [panelId]: _, ...rest } = this.inspectorDismissed;
		this.inspectorDismissed = rest;
	}

	nodes(panelId: string | null): Set<string> {
		return this.sel(panelId).nodes;
	}
	edges(panelId: string | null): Set<string> {
		return this.sel(panelId).edges;
	}

	/** The single selected node of `panelId`, or null for zero / many. */
	selectedNode(panelId: string | null): NodeInstanceInfo | null {
		const ns = this.sel(panelId).nodes;
		if (ns.size !== 1) return null;
		const name = [...ns][0];
		return graph().nodeById(name);
	}

	setActiveEditor(panelId: string): void {
		if (this.activeEditorId !== panelId) this.activeEditorId = panelId;
	}

	/** The ◧'s verb: set the standing preference; turning it on also lifts a dismissal. */
	setInspector(panelId: string, on: boolean): void {
		this.inspectorOn = { ...this.inspectorOn, [panelId]: on };
		if (on) this.undismiss(panelId);
	}
	/** Close the pane until the selection next changes. The ✕'s verb — never the ◧'s. */
	dismissInspectorFor(panelId: string): void {
		this.inspectorDismissed = { ...this.inspectorDismissed, [panelId]: true };
	}
	/** What the pane renders from: the standing preference minus a live dismissal; null is off. */
	inspectorVisibleFor(panelId: string | null): boolean {
		return panelId !== null && (this.inspectorOn[panelId] ?? true) && !this.inspectorDismissed[panelId];
	}

	/** A click adds rather than replaces on a modifier OR while multi-select mode is on; folded in
	 * here, not at each call site, so no caller can forget the mode. */
	clickNode(panelId: string, name: string, modifier: boolean): void {
		const cur = this.sel(panelId);
		const nodes = modifier || this.multiSelect ? toggled(cur.nodes, name) : new Set([name]);
		this.write(panelId, { nodes, edges: cur.edges });
	}

	selectNodes(panelId: string, names: Iterable<string>): void {
		this.write(panelId, { nodes: new Set(names), edges: this.sel(panelId).edges });
	}

	/** Replace both node and edge selection at once (NavContext restore). */
	setSelection(panelId: string, nodes: Iterable<string>, edges: Iterable<string>): void {
		this.write(panelId, { nodes: new Set(nodes), edges: new Set(edges) });
	}

	/** Same fold as `clickNode`; the mode covers edges too, since a plain edge click clears nodes. */
	clickEdge(panelId: string, id: string, modifier: boolean): void {
		const cur = this.sel(panelId);
		if (modifier || this.multiSelect) this.write(panelId, { nodes: cur.nodes, edges: toggled(cur.edges, id) });
		else this.write(panelId, { nodes: new Set(), edges: new Set([id]) });
	}

	/** The same fold, on empty canvas: with the mode on, a stray tap must not wipe the selection. */
	clickPane(panelId: string, shift: boolean): void {
		if (shift || this.multiSelect) return;
		this.clear(panelId);
	}

	clear(panelId: string): void {
		const cur = this.sel(panelId);
		if (cur.nodes.size || cur.edges.size) this.write(panelId, { nodes: new Set(), edges: new Set() });
	}

	/** Drop ALL per-panel state on a layout replace: a loaded `.gfi` keeps its saved panel ids,
	 * which can collide with ids this session already used. */
	forgetAll(): void {
		this.map = {};
		this.inspectorOn = {};
		this.inspectorDismissed = {};
		this.activeEditorId = null;
	}
}

let _sel: SelectionStore | null = null;
export function selection(): SelectionStore {
	if (!_sel) _sel = new SelectionStore();
	return _sel;
}
