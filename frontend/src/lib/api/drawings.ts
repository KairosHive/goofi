/** The main thread's handles on the plot surfaces and drawings the worker owns: a surface is a
 * canvas whose control went to the worker, a drawing a viewer's picture on it. Each posts what
 * changed and hears back what the DOM still shows. */
import { listen, post } from './data';
import type { DrawBox, DrawnState } from '$lib/viewers/drawing';
import type { Hover, ProbeBox } from '$lib/viewers/hover';
import type { ViewerKind } from '$lib/viewers/registry';
import type { SettingsMap } from '$lib/viewers/module';
import type { View } from 'plotluck';

let nextId = 1;

export interface SurfaceHandle {
	readonly id: number;
	/** The pane and the camera; the canvas backing store follows in the worker. */
	setView(v: View): void;
	dispose(): void;
}

export interface DrawingHandle {
	readonly id: number;
	place(x: number, y: number, w: number, h: number, z: number): void;
	setBackground(hex: string): void;
	settings(settings: SettingsMap, box: DrawBox): void;
	/** Feed the drawing from a stream; the worker replays the stream's latest frame at once. */
	attach(node: string, slot: string): void;
	/** Stop feeding it; `keep` leaves the picture standing, as a thumbnail does. */
	detach(keep: boolean): void;
	pointer(at: { x: number; y: number; box: ProbeBox } | null): void;
	drag(dx: number, dy: number, box: ProbeBox): void;
	remove(): void;
}

const lostCbs = new Map<number, () => void>();
const stateCbs = new Map<number, (s: DrawnState) => void>();
const hoverCbs = new Map<number, (h: Hover | null) => void>();

listen((m) => {
	if ('surface' in m) {
		if (!m.surface.ok) lostCbs.get(m.surface.id)?.();
	} else if ('drawn' in m) {
		stateCbs.get(m.drawn.id)?.(m.drawn);
	} else if ('hover' in m) {
		hoverCbs.get(m.hover.id)?.(m.hover.hover);
	}
});

/** Hand `canvas` to the worker as a surface. `onLost` fires when the worker could not open a
 * WebGL2 context on it; the handle is then dead and the host shows the fact. */
export function createSurface(canvas: HTMLCanvasElement, onLost: () => void): SurfaceHandle {
	const id = nextId++;
	lostCbs.set(id, onLost);
	const offscreen = canvas.transferControlToOffscreen();
	post({ op: 'surface', id, canvas: offscreen }, [offscreen]);
	return {
		id,
		setView: (view) => post({ op: 'view', id, view }),
		dispose() {
			lostCbs.delete(id);
			post({ op: 'dispose', id });
		}
	};
}

export function createDrawing(
	surface: SurfaceHandle,
	kind: ViewerKind,
	variant: string,
	onState: (s: DrawnState) => void,
	onHover: (h: Hover | null) => void
): DrawingHandle {
	const id = nextId++;
	stateCbs.set(id, onState);
	hoverCbs.set(id, onHover);
	post({ op: 'drawing', id, surface: surface.id, kind, variant });
	return {
		id,
		place: (x, y, w, h, z) => post({ op: 'place', id, x, y, w, h, z }),
		setBackground: (hex) => post({ op: 'background', id, hex }),
		settings: (settings, box) => post({ op: 'settings', id, settings, box }),
		attach: (node, slot) => post({ op: 'attach', id, node, slot }),
		detach: (keep) => post({ op: 'detach', id, keep }),
		pointer: (at) => post({ op: 'pointer', id, at }),
		drag: (dx, dy, box) => post({ op: 'drag', id, dx, dy, box }),
		remove() {
			stateCbs.delete(id);
			hoverCbs.delete(id);
			post({ op: 'drop', id });
		}
	};
}
