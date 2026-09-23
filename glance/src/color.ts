/** The series palette, procedural and online: series `i` takes the hue that bisects the widest
 * arc left by series 0..i-1, so earlier series never move, every power of two is evenly spaced,
 * and the smallest hue gap never falls under half the even spacing. Rings of eight shift
 * lightness and chroma a step, so a ninth series differs from the first in more than hue. */

const BASE_HUE = 250;
// Mid lightness: many lines over a dark surface stay a picture, not a glare.
const RING: [number, number][] = [
	[0.7, 0.115],
	[0.6, 0.14],
	[0.78, 0.085]
];

/** The greedy widest-arc bisection on the unit circle from 0: the binary van der Corput sequence. */
export function bisect(i: number): number {
	let t = 0;
	for (let d = 0.5; i > 0; i = Math.floor(i / 2), d /= 2) if (i % 2) t += d;
	return t;
}

function lch(i: number): [number, number, number] {
	const [l, c] = RING[Math.floor(i / 8) % RING.length];
	return [l, c, (BASE_HUE + 360 * bisect(i)) % 360];
}

/** Series `i` as `#rrggbb`; `seriesColor(0)` is a blue and no two indices agree. */
export function seriesColor(i: number): string {
	return oklchHex(...lch(i));
}

/** The colour as floats in 0..1, for a GL upload. */
export function seriesRgba(i: number): [number, number, number, number] {
	return oklchRgba(...lch(i));
}

/** OKLCH to sRGB; the chroma is reduced in steps until the colour fits the gamut. */
export function oklchRgba(l: number, c: number, h: number): [number, number, number, number] {
	for (let k = c; k >= 0; k -= 0.005) {
		const rgb = oklabToLinear(l, k * Math.cos((h * Math.PI) / 180), k * Math.sin((h * Math.PI) / 180));
		if (rgb.every((v) => v >= -1e-6 && v <= 1 + 1e-6)) {
			return [gamma(rgb[0]), gamma(rgb[1]), gamma(rgb[2]), 1];
		}
	}
	const g = gamma(oklabToLinear(l, 0, 0)[0]);
	return [g, g, g, 1];
}

export function oklchHex(l: number, c: number, h: number): string {
	const [r, g, b] = oklchRgba(l, c, h);
	return '#' + [r, g, b].map((v) => Math.round(Math.min(1, Math.max(0, v)) * 255).toString(16).padStart(2, '0')).join('');
}

function oklabToLinear(L: number, a: number, b: number): [number, number, number] {
	const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
	const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
	const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
	return [
		4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
		-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
		-0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s
	];
}

function gamma(v: number): number {
	const x = Math.min(1, Math.max(0, v));
	return x <= 0.0031308 ? 12.92 * x : 1.055 * x ** (1 / 2.4) - 0.055;
}
