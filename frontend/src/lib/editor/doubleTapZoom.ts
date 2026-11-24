/** Double-tap, then drag to zoom — the one-handed zoom recognizer and its viewport arithmetic. */
import type { Viewport } from '@xyflow/svelte';
import { eventPoint, type ScreenPoint } from './eventPoint';

/** How long a touch may last and still be a tap. Must stay under the 500 ms long press. */
export const TAP_MS = 300;
/** How long the second tap may take to arrive. */
export const DOUBLE_TAP_MS = 300;
/** How far a tap may travel, and how far the second may land from the first. */
export const TAP_SLOP_PX = 24;
/** The drag distance that doubles (or halves) the zoom. */
export const ZOOM_PX_PER_DOUBLING = 150;

export function createDoubleTapZoom() {
	let press: { p: ScreenPoint; t: number } | null = null;
	let first: { p: ScreenPoint; t: number } | null = null;
	let origin: ScreenPoint | null = null;

	const far = (a: ScreenPoint, b: ScreenPoint): boolean =>
		Math.hypot(a.clientX - b.clientX, a.clientY - b.clientY) > TAP_SLOP_PX;

	return {
		/** True while the zoom gesture is in flight. */
		get active(): boolean {
			return origin !== null;
		},
		/** A finger landed. True if it completed a double tap, which the caller keeps off the panner. */
		down(p: ScreenPoint, now: number): boolean {
			press = { p: { clientX: p.clientX, clientY: p.clientY }, t: now };
			if (!first || now - first.t > DOUBLE_TAP_MS || far(p, first.p)) {
				first = null;
				return false;
			}
			first = null;
			origin = { clientX: p.clientX, clientY: p.clientY };
			return true;
		},
		/** The zoom multiplier against the zoom the gesture started at; null if this is not the gesture. */
		move(p: ScreenPoint): number | null {
			// Measured from the press ORIGIN, so a drift cannot creep past the slop step by step.
			if (press && far(p, press.p)) press = null;
			if (!origin) return null;
			return Math.pow(2, (origin.clientY - p.clientY) / ZOOM_PX_PER_DOUBLING);
		},
		/** A finger lifted: it ends a gesture in flight, or is remembered as a first tap. */
		up(p: ScreenPoint, now: number): void {
			const ended = press;
			press = null;
			if (origin) {
				// The gesture's own finger leaves no tap behind, or a third tap re-arms the zoom.
				origin = null;
				first = null;
				return;
			}
			first = ended && now - ended.t <= TAP_MS && !far(p, ended.p) ? { p: ended.p, t: now } : null;
		},
		/** Drop everything — a cancelled touch, or a second finger arriving for a pinch. */
		cancel(): void {
			press = null;
			first = null;
			origin = null;
		}
	};
}

/** The viewport that scales `from` by `factor` and holds the flow point `anchor` still on screen.
 * The zoom is clamped BEFORE the offset, or the pan derives from a zoom never taken. */
export function zoomStep(
	from: Viewport,
	anchor: { x: number; y: number },
	factor: number,
	limits: { min: number; max: number }
): Viewport {
	const zoom = Math.min(limits.max, Math.max(limits.min, from.zoom * factor));
	return {
		x: from.x + anchor.x * (from.zoom - zoom),
		y: from.y + anchor.y * (from.zoom - zoom),
		zoom
	};
}

/** Wire the double-tap-and-drag zoom onto `root`'s bare pane. On `touchstart` in CAPTURE, not
 * `pointerdown`: Svelte Flow pans by d3-zoom, which binds `touchstart` on its own pane wrapper, so
 * only a capture listener above it can stop the pan. A second finger is a pinch and is handed back.
 * Answers the teardown; a gesture in flight must not keep writing a torn-down viewport. */
export function bindTapZoom(
	root: HTMLElement,
	o: {
		/** Whether a touch at `target` may start the gesture: the bare pane, with nothing pending. */
		allowed: (target: EventTarget | null) => boolean;
		/** Called as a double tap is recognised, so a long-press door armed by the first tap stands down. */
		onStart?: () => void;
		getViewport: () => Viewport | undefined;
		screenToFlow: (p: { x: number; y: number }) => { x: number; y: number } | undefined;
		setViewport: (v: Viewport) => void;
		min: number;
		max: number;
	}
): () => void {
	const tapZoom = createDoubleTapZoom();
	// Sampled ONCE at the start, so the drag cannot accumulate rounding.
	let from: Viewport | null = null;
	let anchor: { x: number; y: number } | null = null;
	const start = (e: TouchEvent): void => {
		if (e.touches.length > 1 || !o.allowed(e.target)) {
			tapZoom.cancel();
			return;
		}
		const p = eventPoint(e);
		if (!p || !tapZoom.down(p, e.timeStamp)) return;
		o.onStart?.();
		from = o.getViewport() ?? null;
		anchor = o.screenToFlow({ x: p.clientX, y: p.clientY }) ?? null;
		e.stopPropagation();
		e.preventDefault();
	};
	const move = (e: TouchEvent): void => {
		const p = eventPoint(e);
		if (!p) return;
		const factor = tapZoom.move(p);
		if (factor === null || !from || !anchor) return;
		e.stopPropagation();
		e.preventDefault();
		o.setViewport(zoomStep(from, anchor, factor, { min: o.min, max: o.max }));
	};
	const end = (e: TouchEvent): void => {
		const p = eventPoint(e);
		if (p) tapZoom.up(p, e.timeStamp);
	};
	const opts = { capture: true, passive: false };
	root.addEventListener('touchstart', start, opts);
	root.addEventListener('touchmove', move, opts);
	root.addEventListener('touchend', end, opts);
	root.addEventListener('touchcancel', tapZoom.cancel, { capture: true });
	return () => {
		root.removeEventListener('touchstart', start, opts);
		root.removeEventListener('touchmove', move, opts);
		root.removeEventListener('touchend', end, opts);
		root.removeEventListener('touchcancel', tapZoom.cancel, { capture: true });
		tapZoom.cancel();
	};
}
