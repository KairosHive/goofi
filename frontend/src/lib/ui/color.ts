/** Colour arithmetic the pickers share: RGBA in 0..1, HSV, CSS and hex text. */

export const unit = (x: number): number => Math.min(1, Math.max(0, Number.isFinite(x) ? x : 0));

/** Hue in degrees, saturation and value from 0 to 1. */
export function toHsv([r, g, b]: number[]): [number, number, number] {
	const max = Math.max(r, g, b);
	const d = max - Math.min(r, g, b);
	let h = 0;
	if (d > 0) {
		if (max === r) h = ((g - b) / d) % 6;
		else if (max === g) h = (b - r) / d + 2;
		else h = (r - g) / d + 4;
		h = (h * 60 + 360) % 360;
	}
	return [h, max > 0 ? d / max : 0, max];
}

export function toRgb(h: number, s: number, v: number): number[] {
	const f = (n: number): number => {
		const k = (n + h / 60) % 6;
		return v - v * s * Math.max(0, Math.min(k, 4 - k, 1));
	};
	return [f(5), f(3), f(1)];
}

export const css = ([r, g, b, a]: number[]): string =>
	`rgba(${Math.round(r * 255)}, ${Math.round(g * 255)}, ${Math.round(b * 255)}, ${a})`;

const byte = (x: number): string => Math.round(unit(x) * 255).toString(16).padStart(2, '0');

/** `#rrggbbaa`, the alpha left off when it is full. */
export const hexOf = (c: number[]): string => '#' + byte(c[0]) + byte(c[1]) + byte(c[2]) + (c[3] < 1 ? byte(c[3]) : '');

/** A typed `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`; anything else is no colour. */
export function parseHex(text: string): number[] | null {
	let h = text.trim().replace(/^#/, '');
	if (h.length === 3 || h.length === 4) h = [...h].map((c) => c + c).join('');
	if (!/^[0-9a-f]{6}([0-9a-f]{2})?$/i.test(h)) return null;
	const n = parseInt(h.padEnd(8, 'f'), 16);
	return [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255].map((b) => b / 255);
}

/** Red, green, blue and alpha each clamped to 0..1, a missing alpha full. */
export const rgbaOf = (value: number[]): number[] => [0, 1, 2, 3].map((i) => unit(i === 3 ? (value[i] ?? 1) : (value[i] ?? 0)));
