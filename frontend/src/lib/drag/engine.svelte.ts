/* The one drag/drop engine. A source (a node on the canvas, a widget, a state box, a palette chip)
 * offers tags; a target (a param row, a widget, a panel, the board) takes tags; the engine pairs the
 * two under the pointer and hands the target the drop. A canvas that owns its gesture (Svelte Flow)
 * feeds the engine its points; `draggable` does it for a pointer press. Inside a source's `home` the
 * drag is the owner's own move and no target is sought, so ONE gesture repositions within a panel
 * and references beyond it. */

/** What flows across a drop: a node to bind, a value expression of a form, a widget kind to bear. */
export type DragTag = 'node' | 'number' | 'text' | 'widget';

export interface Point {
	x: number;
	y: number;
}

export interface DragSource {
	/** What is dragged, as the taker reads it: a node's uid, a variable's name, a widget kind. */
	key: string;
	/** What the source offers. A target takes the first of its own tags the source offers. */
	tags: readonly DragTag[];
	/** The ghost's text; a source with none draws its own ghost, or nothing. */
	label?: string;
	/** Inside it the drag is the owner's own move, and no target is sought. */
	home?: Element | null;
	/** The expression a target that takes `tag` writes; null declines. It may ask the user first. */
	expression?(tag: DragTag, at: Point): string | null | Promise<string | null>;
}

export interface DropTarget {
	el: Element;
	/** A name a hint or an owner looks the target up by. */
	id?: string;
	/** What the target takes, in order of preference. */
	tags: readonly DragTag[];
	/** An exception the tags cannot state, such as a node's own params. */
	accept?(source: DragSource): boolean;
	onDrop(drop: Drop): void;
}

export interface Drop {
	source: DragSource;
	/** The tag the target took. */
	tag: DragTag;
	at: Point;
}

/** How a drag ended: where the pointer was let go, and who took it, if anyone. */
export interface Outcome {
	inHome: boolean;
	target: DropTarget | null;
	start: Point;
	at: Point;
}

/** The tag `target` takes from `source`: the first of the target's own the source offers, or null. */
export function takes(target: Pick<DropTarget, 'tags' | 'accept'>, source: DragSource): DragTag | null {
	if (target.accept && !target.accept(source)) return null;
	return target.tags.find((t) => source.tags.includes(t)) ?? null;
}

/** The element above `el`, through a shadow root to its host. */
function above(el: Element): Element | null {
	if (el.parentElement) return el.parentElement;
	const root = el.getRootNode();
	return root instanceof ShadowRoot ? root.host : null;
}

export class DragEngine {
	#targets = new Map<Element, DropTarget>();
	#elementAt: (x: number, y: number) => Element | null;

	/** The drag in flight, or null. */
	source = $state.raw<DragSource | null>(null);
	/** Where it began and where the pointer is, in client pixels. */
	start = $state.raw<Point | null>(null);
	point = $state.raw<Point | null>(null);
	/** The target under the pointer that takes the source. */
	over = $state.raw<DropTarget | null>(null);
	/** Whether the pointer is inside the source's home. */
	inHome = $state(false);

	constructor(elementAt: (x: number, y: number) => Element | null = (x, y) => document.elementFromPoint(x, y)) {
		this.#elementAt = elementAt;
	}

	/** Take a target, and give back its removal. */
	register(target: DropTarget): () => void {
		this.#targets.set(target.el, target);
		return () => {
			if (this.#targets.get(target.el) === target) this.#targets.delete(target.el);
			if (this.over === target) this.over = null;
		};
	}

	/** The target named `id`. */
	target(id: string): DropTarget | null {
		for (const t of this.#targets.values()) if (t.id === id) return t;
		return null;
	}

	/** What the target named `id` would take from the drag in flight; null while nothing would. */
	takes(id: string): DragTag | null {
		const t = this.source && this.target(id);
		return t && this.source ? takes(t, this.source) : null;
	}

	isOver(what: Element | string): boolean {
		return this.over !== null && (typeof what === 'string' ? this.over.id === what : this.over.el === what);
	}

	begin(source: DragSource, at: Point): void {
		this.cancel();
		this.source = source;
		this.start = at;
		window.addEventListener('keydown', this.#onKey, true);
		this.#place(source, at);
	}

	/** Follow the pointer; null once the drag was cancelled. */
	move(at: Point): Outcome | null {
		return this.source ? this.#place(this.source, at) : null;
	}

	/** Let go: the target under the pointer takes the source. Null once the drag was cancelled. */
	drop(at: Point): Outcome | null {
		const source = this.source;
		if (!source) return null;
		const outcome = this.#place(source, at);
		this.#clear();
		if (outcome.target) {
			const tag = takes(outcome.target, source);
			if (tag) outcome.target.onDrop({ source, tag, at });
		}
		return outcome;
	}

	cancel(): void {
		if (this.source) this.#clear();
	}

	#place(source: DragSource, at: Point): Outcome {
		let target: DropTarget | null = null;
		let inHome = false;
		for (let el = this.#elementAt(at.x, at.y); el; el = above(el)) {
			if (el === source.home) {
				inHome = true;
				break;
			}
			if (!target) {
				const t = this.#targets.get(el);
				if (t && takes(t, source)) target = t;
			}
		}
		if (inHome) target = null;
		this.point = at;
		if (this.over !== target) this.over = target;
		if (this.inHome !== inHome) this.inHome = inHome;
		return { inHome, target, start: this.start ?? at, at };
	}

	#clear(): void {
		window.removeEventListener('keydown', this.#onKey, true);
		this.source = null;
		this.start = null;
		this.point = null;
		this.over = null;
		this.inHome = false;
	}

	#onKey = (e: KeyboardEvent): void => {
		if (e.key !== 'Escape') return;
		e.stopPropagation();
		this.cancel();
	};
}

let _drag: DragEngine | null = null;
export function drag(): DragEngine {
	if (!_drag) _drag = new DragEngine();
	return _drag;
}
