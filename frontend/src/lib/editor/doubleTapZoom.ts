/** Double-tap, then drag to zoom — the one-handed zoom recognizer and its viewport arithmetic. */
import type { Viewport } from '@xyflow/svelte';
import type { ScreenPoint } from './eventPoint';

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
