/** Canvas colour palette: canvas 2D/WebGL cannot read the CSS custom properties. */

/** Eight series hues at one OKLCH lightness (0.78) and chroma (0.13), so no series shouts over
 * another on the dark surface; neighbours in the list sit far apart on the hue circle. */
export const SERIES: string[] = [
	'#7dbdfe', // blue
	'#e3ad4b', // amber
	'#61d19a', // mint
	'#f893bc', // rose
	'#c4a4fe', // violet
	'#a4c665', // lime
	'#1acfdf', // cyan
	'#ff987e' // coral
];

export const AXIS_INK = 'rgba(208, 208, 208, 0.55)';

/** The app's mono stack at `px`, ready for `ctx.font` — spelled out, since a canvas
 * context cannot resolve `var(--font-mono)` and discards the whole declaration. */
export function tickFont(px: number): string {
	return `${px}px "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`;
}
