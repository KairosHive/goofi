/** Canvas colour palette: canvas 2D/WebGL cannot read the CSS custom properties. */

export const AXIS_INK = 'rgba(208, 208, 208, 0.55)';

/** The app's mono stack at `px`, ready for `ctx.font` — spelled out, since a canvas
 * context cannot resolve `var(--font-mono)` and discards the whole declaration. */
export function tickFont(px: number): string {
	return `${px}px "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`;
}
