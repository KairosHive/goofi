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

	/** Id of the linkable panel the dragged node is over, or null. */
	nodeDragTarget = $state<string | null>(null);

	/** The `data-node-drop` value of the drop zone the dragged node is over, or null. */
	nodeDragZone = $state<string | null>(null);

	/** Input slots an in-flight cable drag is near ({@link slotKey} keys); replaced, never mutated. */
	cableNear = $state.raw<ReadonlySet<string>>(new Set());

	setCableNear(keys: ReadonlySet<string>): void {
		this.cableNear = keys;
	}

	isCableNear(node: string, slot: string): boolean {
		return this.cableNear.has(slotKey(node, slot));
	}

	/** What a node dropped on a panel or a marked drop zone MEANS, by the OWNER of the target — a
	 * panel id, or whatever a zone's owner minted. The editor knows where a drop landed and never
	 * what it means. Not `$state`: it is read by an event handler, never rendered. */
	#nodeDrops = new Map<string, NodeDrop>();

	/** Take an owner's drop meaning, and give back the undo of that registration. */
	onNodeDrop(owner: string, drop: NodeDrop): () => void {
		this.#nodeDrops.set(owner, drop);
		return () => {
			if (this.#nodeDrops.get(owner) === drop) this.#nodeDrops.delete(owner);
		};
	}

	/** Hand node `uid` to what owns `target`; false when nobody claimed it. A zone names its owner
	 * before the `#`, and what the owner reads after it. */
	dropNode(target: string, uid: string, at: DropPoint): boolean {
		const cut = target.indexOf('#');
		const owner = cut < 0 ? target : target.slice(0, cut);
		const drop = this.#nodeDrops.get(owner);
		if (!drop) return false;
		drop(uid, cut < 0 ? '' : target.slice(cut + 1), at);
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

	requestSlotClick(seed: SlotClickSeed): void {
		this.pendingSlotClick = seed;
	}

	consumeSlotClick(): SlotClickSeed | null {
		const seed = this.pendingSlotClick;
		this.pendingSlotClick = null;
		return seed;
	}
}

let _ui: UIStore | null = null;
export function ui(): UIStore {
	if (!_ui) _ui = new UIStore();
	return _ui;
}
