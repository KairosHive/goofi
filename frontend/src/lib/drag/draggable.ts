/* `use:draggable`: a pointer press on the element lifts a source into the engine once it moves past
 * the slop, and feeds the engine the pointer until it is let go. A press that never moves is a tap;
 * a finger held still is a hold. */
import type { ActionReturn } from 'svelte/action';
import { createLongPress } from 'panelty';
import { drag, type DragSource, type Outcome, type Point } from './engine.svelte';

/** How far a press moves before it is a drag and not a tap. */
const SLOP = 4;

export interface DraggableOptions {
	/** The source a press lifts, read when the drag begins; null means the press is no drag. */
	source: () => DragSource | null;
	/** Which presses count, by the pressed element; every press by default. */
	allow?: (target: Element) => boolean;
	/** An allowed press, before anything moves. */
	onPress?: (e: PointerEvent) => void;
	/** Let go without having moved past the slop. */
	onTap?: (e: PointerEvent) => void;
	/** A finger held still, instead of a drag: the touch door onto a menu. */
	onHold?: (at: Point) => void;
	/** The drag ended: how, or null when it was cancelled. */
	onEnd?: (outcome: Outcome | null) => void;
}

export function draggable(el: HTMLElement, opts: DraggableOptions | null): ActionReturn<DraggableOptions | null> {
	const engine = drag();
	let now = opts;
	let press: { id: number; x: number; y: number; source: DragSource | null; begun: boolean; dead: boolean } | null = null;
	const hold = createLongPress((at) => {
		const o = now;
		press = null;
		o?.onHold?.({ x: at.clientX, y: at.clientY });
	});

	function down(e: PointerEvent): void {
		const o = now;
		if (!o || e.button !== 0 || press) return;
		if (o.allow && !o.allow(e.target as Element)) return;
		press = { id: e.pointerId, x: e.clientX, y: e.clientY, source: null, begun: false, dead: false };
		el.setPointerCapture(e.pointerId);
		if (e.pointerType !== 'mouse' && o.onHold) hold.start(e);
		o.onPress?.(e);
	}

	function move(e: PointerEvent): void {
		if (!press || press.id !== e.pointerId || press.dead) return;
		hold.move(e);
		const at = { x: e.clientX, y: e.clientY };
		if (!press.begun) {
			if (Math.hypot(at.x - press.x, at.y - press.y) < SLOP) return;
			hold.cancel();
			press.source = now?.source() ?? null;
			if (!press.source) {
				press.dead = true;
				return;
			}
			press.begun = true;
			engine.begin(press.source, { x: press.x, y: press.y });
		}
		// Null: the engine let the drag go (Escape) under a press that is still down.
		if (engine.move(at) === null) end(null);
	}

	function up(e: PointerEvent): void {
		if (!press || press.id !== e.pointerId) return;
		const o = now;
		const p = press;
		if (!p.begun) {
			press = null;
			hold.cancel();
			if (!p.dead) o?.onTap?.(e);
			return;
		}
		end(engine.drop({ x: e.clientX, y: e.clientY }));
	}

	function cancel(e: PointerEvent): void {
		if (!press || press.id !== e.pointerId) return;
		if (press.begun) {
			engine.cancel();
			end(null);
		} else {
			press = null;
			hold.cancel();
		}
	}

	/** The drag is over: tell the owner once, and forget the press. */
	function end(outcome: Outcome | null): void {
		if (!press) return;
		press = null;
		hold.cancel();
		now?.onEnd?.(outcome);
	}

	el.addEventListener('pointerdown', down);
	el.addEventListener('pointermove', move);
	el.addEventListener('pointerup', up);
	el.addEventListener('pointercancel', cancel);
	el.addEventListener('lostpointercapture', cancel);
	return {
		update(next) {
			now = next;
		},
		destroy() {
			el.removeEventListener('pointerdown', down);
			el.removeEventListener('pointermove', move);
			el.removeEventListener('pointerup', up);
			el.removeEventListener('pointercancel', cancel);
			el.removeEventListener('lostpointercapture', cancel);
			if (press?.begun) engine.cancel();
			press = null;
			hold.cancel();
		}
	};
}
