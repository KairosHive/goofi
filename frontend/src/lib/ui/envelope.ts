/** An ADSR envelope as the widgets draw it: the four stages, and the outline they make. */

/** The stages, in the order the four numbers hold them. */
export const ADSR = ['attack', 'decay', 'sustain', 'release'] as const;
export const ADSR_SHORT = ['A', 'D', 'S', 'R'] as const;

/** The four numbers a fresh envelope holds; `goofi_core::ENVELOPE_BORN` says the same. */
export const ENVELOPE_BORN: number[] = [0.01, 0.1, 1, 0.3];

/** `adsr` with stage `i` replaced, the rest kept. */
export const withStage = (adsr: number[], i: number, v: number): number[] =>
	[0, 1, 2, 3].map((k) => (k === i ? v : adsr[k] ?? (k === 2 ? 1 : 0)));

/** A release of -1 holds the sustain for ever. */
export const HOLD = -1;

/** A stage's reading: seconds for the times, a percentage for the sustain, ∞ for a hold. */
export function readout(i: number, v: number): string {
	if (i === 2) return `${Math.round(v * 100)}%`;
	return v < 0 ? '∞' : `${v.toFixed(2)}s`;
}

/** The outline in a `w × h` box: up over the attack, down to the sustain over the decay, then
 * down over the release; a hold runs at the sustain to the edge. The times share the width. */
export function envelopePoints([a, d, s, r]: number[], w: number, h: number): [number, number][] {
	const held = r < 0;
	const times = Math.max(a + d + (held ? 0 : r), 1e-6);
	const hold = held ? times : times / 3;
	const k = w / (times + hold);
	const level = (x: number): number => h - Math.min(1, Math.max(0, x)) * h;
	const tail: [number, number][] = held ? [[w, level(s)], [w, h]] : [[(a + d + hold) * k, level(s)], [w, h]];
	return [[0, h], [a * k, level(1)], [(a + d) * k, level(s)], ...tail];
}

/** `points` as one SVG polyline path. */
export const pathOf = (points: [number, number][]): string =>
	points.map(([x, y], i) => `${i === 0 ? 'M' : 'L'}${x.toFixed(2)} ${y.toFixed(2)}`).join(' ');
