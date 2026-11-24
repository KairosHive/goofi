/** A colormap widget's stops, as the `colormap` list section stores them: each slot an `at`, a
 * `colour` and the `blend` to the next stop. The sampling here is the shader's, so the bar the
 * inspector draws is the map the node reads. */
import { STOPS } from '$lib/viewers/colormaps';

export type Blend = 'linear' | 'smooth' | 'step';
export const BLENDS: readonly Blend[] = ['linear', 'smooth', 'step'];

export interface Stop {
	/** Where the stop sits on the map, from 0 to 1. */
	at: number;
	/** Red, green, blue and alpha, each from 0 to 1. */
	colour: number[];
	/** How the colour runs to the next stop. */
	blend: Blend;
}

const clamp = (x: number): number => Math.min(1, Math.max(0, x));

/** The colour at `t`: the stops in their order, the first at or past `t` ending the segment it is
 * in, blended from the one before as that stop's blend says; past both ends the end holds. */
export function sample(stops: Stop[], t: number): number[] {
	const n = stops.length;
	if (n === 0) return [0, 0, 0, 1];
	for (let i = 0; i < n; i++) {
		if (t > stops[i].at) continue;
		if (i === 0) return stops[0].colour;
		const span = Math.max(stops[i].at - stops[i - 1].at, 1e-6);
		let f = (t - stops[i - 1].at) / span;
		if (stops[i - 1].blend === 'smooth') f = f * f * (3 - 2 * f);
		else if (stops[i - 1].blend === 'step') f = 0;
		return stops[i - 1].colour.map((a, k) => a + (stops[i].colour[k] - a) * f);
	}
	return stops[n - 1].colour;
}

const css = (c: number[]): string => `rgba(${Math.round(c[0] * 255)}, ${Math.round(c[1] * 255)}, ${Math.round(c[2] * 255)}, ${c[3]})`;

/** The map as a CSS gradient, sampled finely enough that a smooth blend reads as one. */
export function gradient(stops: Stop[], samples = 64): string {
	const parts: string[] = [];
	for (let i = 0; i <= samples; i++) {
		const t = i / samples;
		parts.push(`${css(sample(stops, t))} ${(t * 100).toFixed(2)}%`);
	}
	return `linear-gradient(to right, ${parts.join(', ')})`;
}

/** The named viewer colormap as stops, evenly spaced and blended straight; none for a name the
 * viewers do not draw. */
export function preset(name: string): Stop[] | null {
	const rgb = STOPS[name];
	if (!rgb) return null;
	return rgb.map((c, i) => ({ at: i / (rgb.length - 1), colour: [c[0] / 255, c[1] / 255, c[2] / 255, 1], blend: 'linear' }));
}

/** The names of the presets, in the viewers' menu order. */
export const PRESETS = Object.keys(STOPS);

/** The stops in order of their place, the one at a tie keeping its order. */
export const sorted = (stops: Stop[]): Stop[] => [...stops].sort((a, b) => a.at - b.at);

/** A stop opened at `t`, the colour the map had there, in its place among the others. */
export function inserted(stops: Stop[], t: number): Stop[] {
	const at = clamp(t);
	return sorted([...stops, { at, colour: sample(stops, at), blend: 'linear' }]);
}

/** The middle of the widest gap between two stops: where a new stop has the most room. */
export function widestGap(stops: Stop[]): number {
	const s = sorted(stops);
	let best = 0.5;
	let width = -1;
	for (let i = 1; i < s.length; i++) {
		if (s[i].at - s[i - 1].at > width) {
			width = s[i].at - s[i - 1].at;
			best = (s[i].at + s[i - 1].at) / 2;
		}
	}
	return best;
}
