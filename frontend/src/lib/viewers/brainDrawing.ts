/** One value per channel on the scalp, or the connectivity between channels: a topomap, a ring of
 * chords, or the electrodes in 3-D, turned by a drag. A field plot under paths; names are DOM text. */
import type { ArrayData, DataFrame } from '$lib/codec/decode';
import { extent, paths, seriesColor, type FieldPlot, type PathBuilder, type PathPlot, type Rgba, type Surface } from 'plotluck';
import { project, type Camera } from './brain3d';
import { makeLUTCache } from './colormaps';
import { Base, type DrawBox, type PlacedText } from './drawing';
import { electrodeAt, HEAD_RADIUS, lift } from './eegLayout';
import { formatTick } from './format';
import { axisNames, type Drag, type Probe } from './hover';
import type { SettingsMap } from './module';
import { buildLayout, evaluateAt, fieldCentres, headFrame, solveWeights, type TopoLayout } from './topomapInterp';

/** The radius of a drawn electrode dot, in px. */
const DOT = 2;
/** The steps a bowed chord is drawn in. */
const STEPS = 16;
const INK: Rgba = [230 / 255, 233 / 255, 240 / 255, 1];
const HEAD: Rgba = [197 / 255, 200 / 255, 214 / 255, 1];
const SCAFFOLD: Rgba = [160 / 255, 168 / 255, 184 / 255, 0.25];
const ELECTRODE: Rgba = [14 / 255, 16 / 255, 20 / 255, 1];

/** A channel the layout knows, with its index in the frame. */
interface Channel {
	i: number;
	name: string;
	pos: [number, number];
}
function known(names: string[]): Channel[] {
	const out: Channel[] = [];
	for (let i = 0; i < names.length; i++) {
		const pos = electrodeAt(names[i]);
		if (pos) out.push({ i, name: names[i], pos });
	}
	return out;
}

/** One chord of a (C, C) frame: the upper triangle, read against the window as `t`. */
interface Edge {
	a: number;
	b: number;
	value: number;
	t: number;
}

/** The window the values are read against: their own, or the one asked for. */
function valueWindow(values: number[], s: SettingsMap): [number, number] {
	if (s.auto === false) return [Number(s.vmin), Number(s.vmax)];
	const e = extent(values);
	return e && e[1] > e[0] ? e : [-1, 1];
}

/** The point of `pts` nearest `(px, py)` within `r`, with its squared distance. */
function nearest<T extends { x: number; y: number }>(pts: T[], px: number, py: number, r: number): { p: T; d: number } | null {
	let best: { p: T; d: number } | null = null;
	for (const p of pts) {
		const d = (p.x - px) ** 2 + (p.y - py) ** 2;
		if (d <= r * r && (!best || d <= best.d)) best = { p, d };
	}
	return best;
}

/** A quadratic Bézier from `p` to `q` about `c`, at `t`, in any number of dimensions. */
const bow = (p: number[], c: number[], q: number[], t: number): number[] =>
	p.map((v, j) => (1 - t) ** 2 * v + 2 * (1 - t) * t * c[j] + t * t * q[j]);

function lutRgba(L: Uint8Array, t: number, alpha: number): Rgba {
	const i = ((t * 255) | 0) * 3;
	return [L[i] / 255, L[i + 1] / 255, L[i + 2] / 255, alpha];
}

type BrainMode = 'topomap' | 'ring' | '3d';

/** What the brain viewer draws for a frame of `ndim` axes: its setting, or under `auto` a
 * topomap of one value per channel and a ring of a channel-by-channel matrix. */
export function brainMode(settings: SettingsMap, ndim: number): BrainMode {
	const mode = settings.mode;
	if (mode === 'topomap' || mode === 'ring' || mode === '3d') return mode;
	return ndim <= 1 ? 'topomap' : 'ring';
}

export class BrainDrawing extends Base {
	readonly plots: [FieldPlot, PathPlot];
	private readonly field: FieldPlot;
	private readonly path: PathPlot;
	private readonly lutFor = makeLUTCache();
	private applied: SettingsMap | null = null;
	private last: { frame: DataFrame; settings: SettingsMap; box: DrawBox } | null = null;
	// Turned by the drag and kept while the viewer lives; it is a view, not a setting.
	private camera: Camera = { yaw: 0, pitch: 0.6 };
	private layout: TopoLayout | null = null;
	private weights: Float64Array | null = null;
	// The electrodes the last paint placed, in the unit square, for the hover to name.
	private electrodes: { name: string; x: number; y: number; value: number }[] = [];
	// The points the last ring or 3-D paint placed, in layout px, for the hover to name.
	private placed: { name: string; x: number; y: number; i: number }[] = [];
	private edges: Edge[] = [];
	private names: string[] = [];

	constructor(surface: Surface) {
		super();
		this.field = surface.addField();
		this.path = surface.addPath();
		this.plots = [this.field, this.path];
	}

	/** The field carries the background; the paths lie over it. */
	setBackground(hex: string): void {
		this.field.setBackground(hex);
		this.path.setBackground('#0000');
	}

	push(frame: DataFrame, settings: SettingsMap, box: DrawBox): void {
		this.last = { frame, settings, box };
		const arr = frame.data as ArrayData;
		const mode = brainMode(settings, arr.shape.length);
		if (settings !== this.applied) {
			this.field.setSettings({ lut: this.lutFor(String(settings.colormap ?? 'magma')), bands: settings.contours ? 10 : 0 });
			this.applied = settings;
		}
		const channels = known(axisNames(frame.meta, 0) ?? []);
		this.message = null;
		this.texts = [];
		if (mode === 'topomap') this.paintTopomap(arr, channels, settings, box);
		else if (channels.length < 2) this.fail('need ≥ 2 recognized channels');
		else {
			this.field.clear();
			this.edges = this.edgesOf(arr, channels, settings);
			this.names = channels.map((c) => c.name);
			const L = this.lutFor(String(settings.colormap ?? 'magma'));
			if (mode === 'ring') this.paintRing(channels, box, L);
			else this.paint3d(channels, settings, box, L);
		}
		this.probe = mode === 'topomap' ? this.topomapProbe : this.pointProbe;
		this.drag = mode === '3d' ? this.rotate : null;
	}

	clear(): void {
		super.clear();
		this.last = null;
		this.weights = null;
		this.placed = [];
	}

	private fail(msg: string): void {
		this.clear();
		this.message = msg;
	}

	// ── topomap ──────────────────────────────────────────────────────────────────────────────
	private paintTopomap(arr: ArrayData, channels: Channel[], s: SettingsMap, box: DrawBox): void {
		const { w, h } = box;
		const vals = arr.values as ArrayLike<number>;
		this.electrodes = channels.map((c) => ({ name: c.name, x: c.pos[0], y: c.pos[1], value: Number(vals[c.i]) }));
		if (channels.length === 0) return this.fail('no recognized channels');
		if (channels.length < 3) return this.fail('need ≥ 3 channels for topomap');
		const knownPos = channels.map((c) => c.pos);
		const layoutKey = knownPos.map((p) => `${p[0]},${p[1]}`).join('|');
		if (!this.layout || this.layout.layoutKey !== layoutKey) {
			try {
				this.layout = buildLayout(knownPos, layoutKey);
			} catch {
				this.layout = null;
			}
		}
		if (!this.layout) return this.fail('topomap layout failed');
		const values = channels.map((c) => Number(vals[c.i]));
		this.weights = solveWeights(this.layout, new Float64Array(values));
		const { cx, cy, side, radius } = headFrame(w, h);
		const [lo, hi] = valueWindow(values, s);
		this.field.push({
			...fieldCentres(this.layout, this.weights),
			frame: { x: (cx - side / 2) / w, y: (cy - side / 2) / h, w: side / w, h: side / h },
			disc: { x: 0.5, y: 0.5, r: HEAD_RADIUS },
			lo,
			hi
		});
		// The head's outline and nose, and the electrodes as dots.
		const p = paths();
		const ring: number[] = [];
		for (let k = 0; k <= 64; k++) {
			const a = (k / 64) * Math.PI * 2;
			ring.push((cx + Math.cos(a) * radius) / w, (cy + Math.sin(a) * radius) / h);
		}
		p.stroke(ring, 1.5, HEAD);
		p.stroke([(cx - 8) / w, (cy - radius) / h, cx / w, (cy - radius - 10) / h, (cx + 8) / w, (cy - radius) / h], 1.5, HEAD);
		for (const q of knownPos) p.dot((cx + (q[0] - 0.5) * side) / w, (cy + (q[1] - 0.5) * side) / h, 2 * DOT, ELECTRODE);
		this.path.push(p.build());
	}

	// The hover names an electrode within reach, else reads the field inside the head disc; it
	// says nothing outside the disc. The mark grows as the pointer nears, full-size over the dot.
	private readonly topomapProbe: Probe = (px, py, box) => {
		const layout = this.layout;
		const weights = this.weights;
		if (!layout || !weights) return null;
		const { cx, cy, side, radius } = headFrame(box.w, box.h);
		if ((px - cx) ** 2 + (py - cy) ** 2 > radius * radius) return null;
		const reach = box.tol * 3;
		const at = this.electrodes.map((e) => ({ ...e, x: cx + (e.x - 0.5) * side, y: cy + (e.y - 0.5) * side }));
		const hit = nearest(at, px, py, reach);
		if (hit) {
			const t = Math.min(1, Math.max(0, (Math.sqrt(hit.d) - DOT) / (reach - DOT)));
			const r = DOT + (6 - DOT) * (1 - t);
			return { mark: { x: hit.p.x, y: hit.p.y, r }, lines: [[formatTick(hit.p.value)], [hit.p.name]] };
		}
		const v = evaluateAt(layout, weights, 0.5 + (px - cx) / side, 0.5 + (py - cy) / side);
		return { mark: null, lines: [[formatTick(v)]] };
	};

	// ── connectivity ─────────────────────────────────────────────────────────────────────────
	/** The strongest `top` percent of the edges between the channels drawn, weakest first so the
	 * strong ones land on top. The window is the kept edges' own, so the colours span them. */
	private edgesOf(arr: ArrayData, channels: Channel[], s: SettingsMap): Edge[] {
		// The share of the edges drawn, strongest first: 100 is all of them.
		const top = Math.min(100, Math.max(1, Number(s.top ?? 15)));
		const n = arr.shape[0];
		const v = arr.values as ArrayLike<number>;
		const out: Edge[] = [];
		for (let a = 0; a < channels.length; a++) {
			for (let b = a + 1; b < channels.length; b++) {
				const value = Number(v[channels[a].i * n + channels[b].i]);
				if (Number.isFinite(value)) out.push({ a, b, value, t: 0 });
			}
		}
		out.sort((x, y) => x.value - y.value);
		const kept = out.slice(out.length - Math.max(1, Math.round((out.length * top) / 100)));
		const [lo, hi] = valueWindow(kept.map((e) => e.value), s);
		const span = hi - lo || 1;
		for (const e of kept) e.t = Math.min(1, Math.max(0, (e.value - lo) / span));
		return kept;
	}

	/** A chord as the surface takes it: its width and colour from `t`. */
	private chord(p: PathBuilder, pts: number[], L: Uint8Array, t: number): void {
		p.stroke(pts, 0.5 + 1.5 * t, lutRgba(L, t, 0.15 + 0.85 * t));
	}

	/** A channel's strongest partner, for the readout. */
	private strongest(k: number): Edge | null {
		let best: Edge | null = null;
		for (const e of this.edges) if ((e.a === k || e.b === k) && (!best || e.t > best.t)) best = e;
		return best;
	}

	private paintRing(channels: Channel[], box: DrawBox, L: Uint8Array): void {
		const { w, h } = box;
		const n = channels.length;
		const cx = w / 2;
		const cy = h / 2;
		// The labels take the outer band; the blocks sit inside it and the chords inside them.
		const label = Math.min(64, Math.max(24, Math.min(w, h) * 0.2));
		const radius = Math.max(8, Math.min(w, h) / 2 - label);
		const block = Math.max(3, Math.min(8, (Math.PI * radius) / n - 1));
		// Channel 0 at the top, then clockwise, as a viewer reads a ring.
		const angle = (k: number) => -Math.PI / 2 + (k / n) * Math.PI * 2;
		this.placed = channels.map((c, k) => ({
			name: c.name,
			x: cx + Math.cos(angle(k)) * radius,
			y: cy + Math.sin(angle(k)) * radius,
			i: k
		}));
		const p = paths();
		// The bend's control point sits between the centre and the chord's middle, so neighbours
		// bow near the rim and only a chord across the ring passes the centre.
		const PULL = 0.55;
		for (const e of this.edges) {
			const a = this.placed[e.a];
			const b = this.placed[e.b];
			const m = [cx + ((a.x + b.x) / 2 - cx) * PULL, cy + ((a.y + b.y) / 2 - cy) * PULL];
			const pts: number[] = [];
			for (let k = 0; k <= STEPS; k++) {
				const [x, y] = bow([a.x, a.y], m, [b.x, b.y], k / STEPS);
				pts.push(x / w, y / h);
			}
			this.chord(p, pts, L, e.t);
		}
		const texts: PlacedText[] = [];
		for (let k = 0; k < n; k++) {
			const a = angle(k);
			const dx = Math.cos(a);
			const dy = Math.sin(a);
			// A block of the channel's colour on the rim, as a thick stroke along the radius.
			p.stroke(
				[(cx + dx * radius) / w, (cy + dy * radius) / h, (cx + dx * (radius + block * 1.5)) / w, (cy + dy * (radius + block * 1.5)) / h],
				block,
				seriesColor(k)
			);
			// Text reads outward on the right half and is flipped on the left, never upside down.
			const flip = dx < 0;
			const r = radius + block * 1.5 + 4;
			texts.push({ text: channels[k].name, x: cx + dx * r, y: cy + dy * r, angle: flip ? a + Math.PI : a, align: flip ? 'right' : 'left' });
		}
		this.texts = texts;
		this.path.push(p.build());
	}

	private readonly pointProbe: Probe = (px, py, box) => {
		const near = nearest(this.placed, px, py, box.tol)?.p;
		if (!near) return null;
		const e = this.strongest(near.i);
		const lines = [[near.name]];
		if (e) lines.unshift([formatTick(e.value)], [this.names[e.a === near.i ? e.b : e.a]]);
		return { mark: { x: near.x, y: near.y, r: 4 }, lines };
	};

	// ── 3-D ──────────────────────────────────────────────────────────────────────────────────
	private readonly rotate: Drag = (dx, dy) => {
		this.camera = {
			yaw: this.camera.yaw + dx * 0.01,
			pitch: Math.max(-Math.PI / 2, Math.min(Math.PI / 2, this.camera.pitch + dy * 0.01))
		};
		const l = this.last;
		if (l) this.push(l.frame, l.settings, l.box);
	};

	private paint3d(channels: Channel[], s: SettingsMap, box: DrawBox, L: Uint8Array): void {
		const { w, h } = box;
		// How far a 3-D edge dips into the head: 0 is the straight chord, 1 a bow through the centre.
		const curve = Math.min(1, Math.max(0, Number(s.curve ?? 0.1)));
		const view = project(this.camera, w, h);
		const at = (q: number[]): [number, number] => {
			const v = view(q as [number, number, number]);
			return [v.x / w, v.y / h];
		};
		const p = paths();
		// The equator and two great circles through the vertex are the scaffold; the nose is a
		// wedge off the front, so the way the head faces reads from any angle.
		const circles: number[][] = [[], [], []];
		for (let k = 0; k <= 64; k++) {
			const a = (k / 64) * Math.PI * 2;
			circles[0].push(...at([Math.cos(a), Math.sin(a), 0]));
			circles[1].push(...at([0, Math.cos(a), Math.sin(a)]));
			circles[2].push(...at([Math.cos(a), 0, Math.sin(a)]));
		}
		for (const c of circles) p.stroke(c, 1, SCAFFOLD);
		p.stroke([...at([-0.18, 0.98, 0]), ...at([0, 1.3, 0]), ...at([0.18, 0.98, 0]), ...at([-0.18, 0.98, 0])], 2, [INK[0], INK[1], INK[2], 0.9]);
		const lifted = channels.map((c) => lift(c.pos));
		const points = lifted.map(view);
		this.placed = channels.map((c, k) => ({ name: c.name, x: points[k].x, y: points[k].y, i: k }));
		for (const e of this.edges) {
			const pts: number[] = [];
			if (curve === 0) {
				pts.push(points[e.a].x / w, points[e.a].y / h, points[e.b].x / w, points[e.b].y / h);
			} else {
				// A quadratic bow in head space, its control point the chord's middle pulled past
				// the centre: at 1 the bow's own middle is the centre itself.
				const [q, r] = [lifted[e.a], lifted[e.b]];
				const c = q.map((v, j) => ((v + r[j]) / 2) * (1 - 2 * curve));
				for (let k = 0; k <= STEPS; k++) pts.push(...at(bow(q, c, r, k / STEPS)));
			}
			this.chord(p, pts, L, e.t);
		}
		// The far side of the head is drawn smaller and dimmer, which is all the depth a dot needs.
		for (const q of points) p.dot(q.x / w, q.y / h, 2 * (2 + 2 * q.depth), [INK[0], INK[1], INK[2], 0.35 + 0.65 * q.depth]);
		this.path.push(p.build());
	}
}
