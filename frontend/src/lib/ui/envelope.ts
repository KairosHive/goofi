/** An ADSR envelope as the widgets draw it: the four stages, and the outline they make. */

/** The stages, in the order the four numbers hold them. */
export const ADSR = ['attack', 'decay', 'sustain', 'release'] as const;
export const ADSR_SHORT = ['A', 'D', 'S', 'R'] as const;

/** The four numbers a fresh envelope holds; `goofi_core::ENVELOPE_BORN` says the same. */
export const ENVELOPE_BORN: number[] = [0.01, 0.1, 1, 0.3];

/** `adsr` with stage `i` replaced, the rest kept. */
export const withStage = (adsr: number[], i: number, v: number): number[] =>
	[0, 1, 2, 3].map((k) => (k === i ? v : adsr[k] ?? (k === 2 ? 1 : 0)));

/** A stage's reading: seconds for the times, a percentage for the sustain. */
export function readout(i: number, v: number): string {
	return i === 2 ? `${Math.round(v * 100)}%` : `${v.toFixed(2)}s`;
}

/** The outline in a `w × h` box: up over the attack, down to the sustain over the decay, a hold a
 * quarter of the whole, then down over the release. The times share the width in proportion. */
export function envelopePoints([a, d, s, r]: number[], w: number, h: number): [number, number][] {
	const times = Math.max(a + d + r, 1e-6);
	const hold = times / 3;
	const k = w / (times + hold);
	const level = (x: number): number => h - Math.min(1, Math.max(0, x)) * h;
	return [
		[0, h],
		[a * k, level(1)],
		[(a + d) * k, level(s)],
		[(a + d + hold) * k, level(s)],
		[w, h]
	];
}

/** `points` as one SVG polyline path. */
export const pathOf = (points: [number, number][]): string =>
	points.map(([x, y], i) => `${i === 0 ? 'M' : 'L'}${x.toFixed(2)} ${y.toFixed(2)}`).join(' ');
