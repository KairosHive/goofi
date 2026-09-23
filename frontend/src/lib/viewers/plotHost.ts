/** How a viewer finds the surface it draws on and the card it sits in: two Svelte contexts. */
import { getContext, setContext } from 'svelte';
import { createSurface, type Rect, type Surface, type View } from 'glance';

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

export interface SurfaceMount {
	/** Null where WebGL2 is missing; the mount then holds no view. */
	readonly surface: Surface | null;
	setView(v: Omit<View, 'dpr'>): void;
	dispose(): void;
}

/** A surface on `canvas`; the last view is re-applied when the device pixel ratio changes. */
export function mountSurface(canvas: HTMLCanvasElement): SurfaceMount {
	let surface: Surface | null = null;
	try {
		surface = createSurface(canvas);
	} catch (err) {
		console.warn(err);
	}
	let last: Omit<View, 'dpr'> | null = null;
	let media: MediaQueryList | null = null;
	const dpr = () => window.devicePixelRatio || 1;
	const apply = () => {
		if (last) surface?.setView({ ...last, dpr: dpr() });
	};
	// The query matches the ratio of the moment, so it is re-armed after every change.
	const arm = () => {
		media?.removeEventListener('change', changed);
		media = window.matchMedia(`(resolution: ${dpr()}dppx)`);
		media.addEventListener('change', changed);
	};
	function changed(): void {
		arm();
		apply();
	}
	arm();
	return {
		surface,
		setView(v) {
			last = v;
			apply();
		},
		dispose() {
			media?.removeEventListener('change', changed);
			surface?.dispose();
		}
	};
}
