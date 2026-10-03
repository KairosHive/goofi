/** Cross-component UI state, kept out of the graph store so re-renders stay scoped. */
import { ALL_TAB } from '$lib/editor/nodeSearch';

export type SlotClickSeed = {
	node: string;
	slot: string;
	dtype: string;
	/** `'source'` when the user clicked an output port; `'target'` for inputs. */
	side: 'source' | 'target';
	clientX: number;
	clientY: number;
};

/** Where a node was let go, in client coordinates: what a menu the drop opens hangs at. */
export interface DropPoint {
	x: number;
	y: number;
}

/** What a dropped node does to the zone it landed on: the zone's own tail, and the drop point. */
export type NodeDrop = (uid: string, zone: string, at: DropPoint) => void;

/** Compose the key that names a single (node, slot) pair. */
export function slotKey(node: string, slot: string): string {
	return `${node}|${slot}`;
}

/** A drop target's owner, and the zone tail after its `#` (null for a whole panel). */
export function splitDrop(target: string): { owner: string; zone: string | null } {
	const cut = target.indexOf('#');
	return cut < 0 ? { owner: target, zone: null } : { owner: target.slice(0, cut), zone: target.slice(cut + 1) };
}

export class UIStore {
	/** Ids of the in-panel editors that own the keyboard. NOT `$state`: every registrant is an
	 * `$effect`, so a reactive Set makes each a dependency of what it writes — the count is. */
	#editors = new Set<string>();
	#openCount = $state(0);

	/** True while any in-panel editor owns the keyboard, so global shortcuts stand down. */
	get modalOpen(): boolean {
		return this.#openCount > 0;
	}

	/** The add-node menu's facet, kept across openings so a menu opens where the user left it. A
	 * slot click overrides it with that node's engine, and never writes it. */
	paletteTab = $state(ALL_TAB);

	/** Bubbled-up "user clicked an unconnected port" intent, cleared by the consumer. */
	pendingSlotClick = $state<SlotClickSeed | null>(null);

	/** Node being dragged out of an editor to link into a panel, or null. */
	nodeDrag = $state<string | null>(null);

	/** What the dragged node is over: a linkable panel's id or a zone's `data-node-drop`, or null. */
	nodeDragOver = $state<string | null>(null);

	/** Vector params a reader opened as individual entries, by `uid/group/name`; absent is the list. */
	paramView = $state<Record<string, 'individual'>>({});

	/** Variable being dragged from a widget label to a parameter. */
	variableDrag = $state.raw<{ name: string; x: number; y: number; target: Element | null } | null>(null);

	/** Input slots an in-flight cable drag is near ({@link slotKey} keys); replaced, never mutated. */
	cableNear = $state.raw<ReadonlySet<string>>(new Set());

	isCableNear(node: string, slot: string): boolean {
		return this.cableNear.has(slotKey(node, slot));
	}

	/** What a node dropped on a panel or a drop zone means, by the owner of the target. The editor
	 * knows where a drop landed, never what it means; an event handler reads it, so not `$state`. */
	#nodeDrops = new Map<string, NodeDrop>();

	/** Take an owner's drop meaning, and give back the undo of that registration. */
	onNodeDrop(owner: string, drop: NodeDrop): () => void {
		this.#nodeDrops.set(owner, drop);
		return () => {
			if (this.#nodeDrops.get(owner) === drop) this.#nodeDrops.delete(owner);
		};
	}

	/** Hand node `uid` to what owns `target`; false when nobody claimed it. */
	dropNode(target: string, uid: string, at: DropPoint): boolean {
		const { owner, zone } = splitDrop(target);
		const drop = this.#nodeDrops.get(owner);
		if (!drop) return false;
		drop(uid, zone ?? '', at);
		return true;
	}

	/** Register an open in-panel editor by a stable id (idempotent). */
	openEditor(id: string): void {
		if (this.#editors.has(id)) return;
		this.#editors.add(id);
		this.#openCount = this.#editors.size;
	}

	/** Unregister an editor when it collapses or unmounts (idempotent). */
	closeEditor(id: string): void {
		if (!this.#editors.delete(id)) return;
		this.#openCount = this.#editors.size;
	}
}

let _ui: UIStore | null = null;
export function ui(): UIStore {
	if (!_ui) _ui = new UIStore();
	return _ui;
}
