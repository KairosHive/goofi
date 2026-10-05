/* A variable chip lifted off its label and carried to a drop target: the one gesture a control
 * panel's labels and a machine pane's playhead chips share. The drop lands on whatever element
 * marks itself `data-variable-drop`, which hears `variable-expression-drop` with the expression. */
import { createLongPress } from 'panelty';
import { ui } from '$lib/stores/ui.svelte';

/** How far a press moves before it is a drag and not a tap. */
const SLOP = 4;

export interface VariableLift {
	/** Whether this lift holds a chip right now. */
	readonly active: boolean;
	/** Begin on a pointer down over `name`'s label; a held finger fires `onHold` instead. */
	start(e: PointerEvent, name: string): void;
	move(e: PointerEvent): void;
	/** Let go: the target under the pointer takes the expression when `allowed(name)`. */
	drop(e: PointerEvent, allowed?: (name: string) => boolean): void;
	cancel(): void;
}

export function createVariableLift(onHold?: (x: number, y: number, name: string) => void): VariableLift {
	const uiStore = ui();
	let grab = $state<{ name: string; x: number; y: number; pointer: number } | null>(null);
	let pressName = '';
	const press = createLongPress((at) => {
		cancel();
		onHold?.(at.clientX, at.clientY, pressName);
	});

	function start(e: PointerEvent, name: string): void {
		if (e.button !== 0) return;
		if (e.pointerType !== 'mouse' && onHold) {
			pressName = name;
			press.start(e);
		}
		grab = { name, x: e.clientX, y: e.clientY, pointer: e.pointerId };
		(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
		e.stopPropagation();
	}

	function move(e: PointerEvent): void {
		press.move(e);
		if (!grab || grab.pointer !== e.pointerId) return;
		if (!uiStore.variableDrag && Math.hypot(e.clientX - grab.x, e.clientY - grab.y) < SLOP) return;
		uiStore.variableDrag = {
			name: grab.name, x: e.clientX, y: e.clientY,
			target: document.elementFromPoint(e.clientX, e.clientY)?.closest('[data-variable-drop]') ?? null
		};
	}

	function cancel(): void {
		press.cancel();
		grab = null;
		uiStore.variableDrag = null;
	}

	function drop(e: PointerEvent, allowed: (name: string) => boolean = () => true): void {
		if (!grab || grab.pointer !== e.pointerId) return;
		move(e);
		const dropped = uiStore.variableDrag;
		cancel();
		if (dropped && allowed(dropped.name)) {
			dropped.target?.dispatchEvent(new CustomEvent('variable-expression-drop', { detail: `variables.${dropped.name}` }));
		}
	}

	return {
		get active() {
			return grab !== null;
		},
		start,
		move,
		drop,
		cancel
	};
}
