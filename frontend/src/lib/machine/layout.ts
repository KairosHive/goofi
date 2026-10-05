/** The machine canvas's geometry and colours in FLOW units: a card's width is fixed, so a dot's
 * dock is known before the card is measured. */
import type { Node } from '@xyflow/svelte';

/** A state card's width; its height is its name's row. */
export const CARD_W = 200;
/** A card's height before Svelte Flow has measured it. */
export const FALLBACK_H = 44;
/** Where resting dots sit along a card's top edge: the first dot's centre, and the step. */
export const DOCK = { x: 14, step: 16 } as const;
/** A dot's diameter in flow units. */
export const DOT = 12;

/** Where a card sits and how big it is, from the flow node Svelte Flow holds for it. */
export function cardBox(nodes: Node[], id: string): { x: number; y: number; w: number; h: number } | null {
	const n = nodes.find((n) => n.id === id);
	if (!n) return null;
	return { x: n.position.x, y: n.position.y, w: n.measured?.width ?? CARD_W, h: n.measured?.height ?? FALLBACK_H };
}

/** The colours playheads are born with, by their index among the machine's. */
const PALETTE = ['#f59e0b', '#38bdf8', '#a3e635', '#f472b6', '#c084fc', '#fb7185', '#34d399', '#facc15'];

/** A playhead's colour: its own, or the palette's by `index` when it has none. */
export function dotColor(color: string, index: number): string {
	return color || PALETTE[index % PALETTE.length];
}

/** `#rrggbb` or `#rrggbbaa` as red, green, blue and alpha in 0..1. */
export function hexToRgba(hex: string): number[] {
	const m = /^#([0-9a-f]{6})([0-9a-f]{2})?$/i.exec(hex);
	if (!m) return [0, 0, 0, 1];
	const n = parseInt(m[1], 16);
	return [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((c) => c / 255).concat(m[2] ? parseInt(m[2], 16) / 255 : 1);
}

/** Red, green, blue in 0..1 as `#rrggbb`; the alpha is dropped, a dot is opaque. */
export function rgbaToHex([r, g, b]: number[]): string {
	const byte = (c: number): string => Math.round(Math.min(1, Math.max(0, c ?? 0)) * 255).toString(16).padStart(2, '0');
	return `#${byte(r)}${byte(g)}${byte(b)}`;
}
