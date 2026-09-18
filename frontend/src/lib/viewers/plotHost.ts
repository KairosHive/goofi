/** How a viewer finds the surface it draws on and the card it sits in: two Svelte contexts. */
import { getContext, setContext } from 'svelte';
import type { Rect, Surface } from 'glance';

export interface PlotHost {
	readonly surface: Surface | null;
}

/** The element a plot rect is measured inside, and where that element sits in the surface's flow units. */
export interface PlotAnchor {
	readonly x: number;
	readonly y: number;
	readonly z: number;
	readonly el: HTMLElement | null;
}

const HOST = Symbol('plot-host');
const ANCHOR = Symbol('plot-anchor');

export function provideSurface(host: PlotHost): void {
	setContext(HOST, host);
}
export function useSurface(): PlotHost | undefined {
	return getContext(HOST);
}
export function provideAnchor(anchor: PlotAnchor): void {
	setContext(ANCHOR, anchor);
}
export function useAnchor(): PlotAnchor | undefined {
	return getContext(ANCHOR);
}

/** `el`'s box inside `anchor` by the offset chain: layout units, transforms excluded. */
export function offsetIn(el: HTMLElement, anchor: HTMLElement): Rect {
	let x = 0;
	let y = 0;
	for (let e: HTMLElement | null = el; e && e !== anchor; e = e.offsetParent as HTMLElement | null) {
		x += e.offsetLeft;
		y += e.offsetTop;
	}
	return { x, y, w: el.offsetWidth, h: el.offsetHeight };
}
