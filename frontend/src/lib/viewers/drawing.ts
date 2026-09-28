/** One drawing per viewer of an array kind: the plots it holds on the host's surface, and what
 * the feed still shows in the DOM for it — corner labels, placed text, a message, the hover.
 * A drawing lives where its surface does, in the data worker; nothing here touches the DOM. */
import type { ArrayData, DataFrame } from '$lib/codec/decode';
import type { ImagePlot, LinePlot, Plot, Surface } from 'plotluck';
import { axisNames, imageProbe, lineProbe, trajectoryProbe, type Drag, type Probe } from './hover';
import { formatTick } from './format';
import { makeLUTCache } from './colormaps';
import { lineData, pushImage } from './plotFeed';
import type { SettingsMap } from './settingsSchema';
import type { ViewSummary } from './viewMeta';

/** The body the drawing fills: its layout box in CSS px, its width in device px, the flow zoom. */
export interface DrawBox {
	w: number;
	h: number;
	cols: number;
	zoom: number;
}

/** Text the drawing places on its picture, in layout px, turned by `angle` about its anchor. */
export interface PlacedText {
	text: string;
	x: number;
	y: number;
	angle: number;
	align: 'left' | 'right';
	color?: string;
}

/** What the feed shows in the DOM for a drawing, as the worker last reported it. */
export interface DrawnState {
	/** Whether the drawing holds a frame at all; the feed says "no data yet" until it does. */
	has: boolean;
	/** The summary of a frame the kind could not draw, shown in the picture's place. */
	fallback: ViewSummary | null;
	labels: string[];
	texts: PlacedText[];
	message: string | null;
	/** Whether the drawing answers a drag, which the feed then keeps from the card. */
	drag: boolean;
}

export interface Drawing {
	readonly plots: Plot[];
	setBackground(hex: string): void;
	place(x: number, y: number, w: number, h: number, z: number): void;
	/** Draw `frame` under `settings`; what follows reads off what was drawn. */
	push(frame: DataFrame, settings: SettingsMap, box: DrawBox): void;
	/** Drop the picture: the background alone until the next push. */
	clear(): void;
	remove(): void;
	readonly probe: Probe | null;
	readonly drag: Drag | null;
	/** The corner labels: top-left, bottom-left, bottom-right; '' leaves a corner empty. */
	readonly labels: string[];
	readonly texts: PlacedText[];
	/** What the picture could not show, in its place. */
	readonly message: string | null;
}

/** The shared part: rect, order and background over every plot, and empty DOM state. */
export abstract class Base implements Drawing {
	abstract readonly plots: Plot[];
	probe: Probe | null = null;
	drag: Drag | null = null;
	labels: string[] = [];
	texts: PlacedText[] = [];
	message: string | null = null;
	setBackground(hex: string): void {
		for (const p of this.plots) p.setBackground(hex);
	}
	place(x: number, y: number, w: number, h: number, z: number): void {
		this.plots.forEach((p, i) => {
			p.setRect(x, y, w, h);
			// A later plot draws over an earlier one of the same drawing.
			p.setOrder(z + i / this.plots.length);
		});
	}
	abstract push(frame: DataFrame, settings: SettingsMap, box: DrawBox): void;
	clear(): void {
		for (const p of this.plots) p.clear();
		this.probe = null;
		this.labels = [];
		this.texts = [];
		this.message = null;
	}
	remove(): void {
		for (const p of this.plots) p.remove();
	}
}

export class LineDrawing extends Base {
	readonly plots: [LinePlot];
	private applied: SettingsMap | null = null;
	constructor(surface: Surface) {
		super();
		this.plots = [surface.addLine()];
	}
	push(f: DataFrame, s: SettingsMap, box: DrawBox): void {
		const p = this.plots[0];
		if (s !== this.applied) {
			p.setSettings({
				logX: Boolean(s.logX),
				logY: Boolean(s.logY),
				yAuto: s.yAuto !== false,
				yMin: Number(s.yMin ?? -1),
				yMax: Number(s.yMax ?? 1),
				points: Boolean(s.points),
				width: 2,
				alpha: 0.75,
				square: false
			});
			this.applied = s;
		}
		const logX = Boolean(s.logX);
		const data = lineData(f, box.cols, logX);
		p.push(data);
		const r = p.range();
		if (!r) return;
		this.labels = r.scalar
			? ['', formatTick(r.xMin), formatTick(r.xMax)]
			: [formatTick(r.yMax), formatTick(r.yMin), formatTick(r.xMax)];
		const ndim = (f.data as ArrayData).shape.length;
		this.probe = lineProbe(
			data,
			r,
			{ logX, logY: Boolean(s.logY), pad: 2 / box.zoom },
			{ x: axisNames(f.meta, ndim - 1), series: ndim > 1 ? axisNames(f.meta, 0) : null }
		);
	}
}

/** Every pair of rows as one path, row i along x and row j along y, on one square window. */
const MAX_PAIRS = 64;
/** Past this many points the dots along a path would hide it. */
const MAX_DOTTED = 800;

export class TrajectoryDrawing extends Base {
	readonly plots: [LinePlot];
	private applied: SettingsMap | null = null;
	private dotted = false;
	constructor(surface: Surface) {
		super();
		this.plots = [surface.addLine()];
	}
	push(f: DataFrame, s: SettingsMap, box: DrawBox): void {
		const p = this.plots[0];
		const arr = f.data as ArrayData;
		const [n, m] = arr.shape;
		const dotted = Number(s.pointSize ?? 2) > 0 && m <= MAX_DOTTED;
		if (s !== this.applied || dotted !== this.dotted) {
			p.setSettings({
				yAuto: s.yAuto !== false,
				yMin: Number(s.yMin ?? -1),
				yMax: Number(s.yMax ?? 1),
				points: dotted,
				pointWidth: Number(s.pointSize ?? 2),
				width: 1.4,
				alpha: 1,
				square: true
			});
			this.applied = s;
			this.dotted = dotted;
		}
		const v = arr.values;
		const row = (i: number) => v.subarray(i * m, (i + 1) * m);
		const xs: ArrayLike<number>[] = [];
		const rows: ArrayLike<number>[] = [];
		const pairs: [number, number][] = [];
		for (let i = 0; i < n && pairs.length < MAX_PAIRS; i++) {
			for (let j = i + 1; j < n && pairs.length < MAX_PAIRS; j++) {
				pairs.push([i, j]);
				xs.push(row(i));
				rows.push(row(j));
			}
		}
		const data = { rows, xs };
		p.push(data);
		const r = p.range();
		if (!r) return;
		this.labels = [formatTick(r.yMax), formatTick(r.yMin), formatTick(r.xMax)];
		this.probe = trajectoryProbe(data, r, 2 / box.zoom, pairs, axisNames(f.meta, 0));
	}
}

export class ImageDrawing extends Base {
	readonly plots: [ImagePlot];
	private readonly lutFor = makeLUTCache();
	private applied: SettingsMap | null = null;
	constructor(surface: Surface) {
		super();
		this.plots = [surface.addImage()];
	}
	push(f: DataFrame, s: SettingsMap): void {
		const p = this.plots[0];
		if (s !== this.applied) {
			p.setSettings({ lut: this.lutFor(String(s.colormap ?? 'gray')), stretch: s.stretch === true });
			this.applied = s;
		}
		pushImage(p, f, s);
		this.probe = imageProbe(f.data as ArrayData, s.stretch === true, f.meta);
	}
}
