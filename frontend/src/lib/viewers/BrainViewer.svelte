<!-- One value per channel on the scalp, or the connectivity between channels: a topomap, a ring
     of chords, or the electrodes in 3-D, turned by a drag. -->
<script lang="ts">
	import type { DataFrame, ArrayData } from '$lib/codec/decode';
	import type { SettingsMap } from './settingsSchema';
	import { makeLUTCache } from './colormaps';
	import { electrodeAt, lift } from './eegLayout';
	import { brainMode } from './kind';
	import {
		buildLayout,
		buildPixelCache,
		headFrame,
		solveWeights,
		evaluateField,
		evaluateAt,
		type TopoLayout,
		type PixelCache
	} from './topomapInterp';
	import { onDestroy } from 'svelte';
	import { AXIS_INK, tickFont } from './palette';
	import { formatTick } from './format';
	import { seriesColor } from 'glance';
	import type { Drag, Probe } from './hover';
	import { project, type Camera } from './brain3d';

	type Props = { frame: DataFrame; settings?: SettingsMap; probe?: Probe | null; drag?: Drag | null };
	let { frame, settings = {}, probe = $bindable(null), drag = $bindable(null) }: Props = $props();

	const colormap = $derived(String(settings.colormap ?? 'coolwarm'));
	const autoRange = $derived(settings.auto !== false);
	const vmin = $derived(Number(settings.vmin ?? -1));
	const vmax = $derived(Number(settings.vmax ?? 1));
	const contours = $derived(Boolean(settings.contours));
	const mode = $derived(brainMode(settings, (frame.data as ArrayData).shape.length));

	let canvas: HTMLCanvasElement | null = $state(null);
	let resizer: ResizeObserver | null = null;
	let size = $state({ w: 200, h: 200 });
	const lutFor = makeLUTCache();
	/** The radius of a drawn electrode dot, in px. */
	const DOT = 2;

	// A channel the layout knows, with its index in the frame.
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

	/** The window the values are read against: their own, or the one asked for. */
	function valueWindow(values: Iterable<number>): [number, number] {
		let lo = vmin;
		let hi = vmax;
		if (autoRange) {
			lo = Infinity;
			hi = -Infinity;
			for (const v of values) {
				if (!Number.isFinite(v)) continue;
				if (v < lo) lo = v;
				if (v > hi) hi = v;
			}
			if (!(hi > lo)) {
				lo = -1;
				hi = 1;
			}
		}
		return [lo, hi];
	}

	function drawMessage(ctx: CanvasRenderingContext2D, w: number, h: number, msg: string): void {
		ctx.fillStyle = '#1c2029';
		ctx.fillRect(0, 0, w, h);
		ctx.fillStyle = '#9aa3b3';
		ctx.font = tickFont(11);
		ctx.textAlign = 'center';
		ctx.fillText(msg, w / 2, h / 2);
	}

	// ── topomap ──────────────────────────────────────────────────────────────────────────────
	// The field is evaluated on a grid of at most this many cells a side, then drawn scaled: its
	// cost is per grid cell and per channel, so it must not grow with the canvas.
	const GRID = 128;
	let grid: HTMLCanvasElement | null = null;
	let imageData: ImageData | null = null;
	let layout: TopoLayout | null = null;
	let pixelCache: PixelCache | null = null;
	let field: Float32Array | null = null;
	let weights: Float64Array | null = null;
	// The electrodes the last paint placed, in the unit square, for the hover to name.
	let electrodes: { name: string; x: number; y: number; value: number }[] = [];

	function drawHeadDecor(ctx: CanvasRenderingContext2D, w: number, h: number, channels: Array<[number, number]>): void {
		const { cx, cy, side, radius } = headFrame(w, h);
		ctx.strokeStyle = '#c5c8d6';
		ctx.lineWidth = 1.5;
		ctx.beginPath();
		ctx.arc(cx, cy, radius, 0, Math.PI * 2);
		ctx.stroke();
		ctx.beginPath();
		ctx.moveTo(cx - 8, cy - radius);
		ctx.lineTo(cx, cy - radius - 10);
		ctx.lineTo(cx + 8, cy - radius);
		ctx.stroke();
		ctx.fillStyle = '#0e1014';
		for (const p of channels) {
			ctx.beginPath();
			ctx.arc(cx + (p[0] - 0.5) * side, cy + (p[1] - 0.5) * side, DOT, 0, Math.PI * 2);
			ctx.fill();
		}
	}

	function paintTopomap(ctx: CanvasRenderingContext2D, arr: ArrayData, channels: Channel[]): void {
		const w = size.w;
		const h = size.h;
		const vals = arr.values as ArrayLike<number>;
		electrodes = channels.map((c) => ({ name: c.name, x: c.pos[0], y: c.pos[1], value: Number(vals[c.i]) }));
		if (channels.length === 0) {
			weights = null;
			drawMessage(ctx, w, h, 'no recognized channels');
			return;
		}
		if (channels.length < 3) {
			weights = null;
			drawMessage(ctx, w, h, 'need ≥ 3 channels for topomap');
			return;
		}
		const knownPos = channels.map((c) => c.pos);
		const layoutKey = knownPos.map((p) => `${p[0]},${p[1]}`).join('|');
		if (!layout || layout.layoutKey !== layoutKey) {
			try {
				layout = buildLayout(knownPos, layoutKey);
			} catch {
				layout = null;
			}
			pixelCache = null;
			field = null;
		}
		if (!layout) {
			drawMessage(ctx, w, h, 'topomap layout failed');
			return;
		}
		// The grid keeps the canvas aspect, so the scale is uniform and the head stays a circle.
		const gw = Math.max(1, Math.round((GRID * w) / Math.max(w, h)));
		const gh = Math.max(1, Math.round((GRID * h) / Math.max(w, h)));
		if (!pixelCache || pixelCache.width !== gw || pixelCache.height !== gh || pixelCache.layoutKey !== layoutKey) {
			pixelCache = buildPixelCache(layout, gw, gh);
			field = new Float32Array(pixelCache.count);
		}
		if (!field) return;
		grid ??= document.createElement('canvas');
		if (grid.width !== gw) grid.width = gw;
		if (grid.height !== gh) grid.height = gh;
		const gctx = grid.getContext('2d');
		if (!gctx) return;

		weights = solveWeights(layout, new Float64Array(channels.map((c) => Number(vals[c.i]))));
		evaluateField(layout, pixelCache, weights, field);

		if (!imageData || imageData.width !== gw || imageData.height !== gh) {
			imageData = gctx.createImageData(gw, gh);
		}
		const data = imageData.data;
		data.fill(0);
		const offsets = pixelCache.pixelByteOffsets;
		const count = pixelCache.count;
		const L = lutFor(colormap);
		const [lo, hi] = valueWindow(field.subarray(0, count));
		const span = hi - lo || 1;
		// Contours posterize the normalized value into bands, so the boundaries read as iso-lines.
		for (let p = 0; p < count; p++) {
			const off = offsets[p];
			let t = (field[p] - lo) / span;
			t = t < 0 ? 0 : t > 1 ? 1 : t;
			if (contours) t = Math.round(t * 10) / 10;
			const idx = ((t * 255) | 0) * 3;
			data[off] = L[idx];
			data[off + 1] = L[idx + 1];
			data[off + 2] = L[idx + 2];
			data[off + 3] = 255;
		}
		gctx.putImageData(imageData, 0, 0);
		ctx.clearRect(0, 0, w, h);
		// The field reaches past the head circle, and the circle itself is the clip: an
		// anti-aliased edge everywhere, with no cell left unpainted inside it.
		const head = headFrame(w, h);
		ctx.save();
		ctx.beginPath();
		ctx.arc(head.cx, head.cy, head.radius, 0, Math.PI * 2);
		ctx.clip();
		ctx.imageSmoothingEnabled = true;
		ctx.drawImage(grid, 0, 0, w, h);
		ctx.restore();
		drawHeadDecor(ctx, w, h, knownPos);
	}

	// The hover names an electrode within reach, else reads the field inside the head disc; it
	// says nothing outside the disc. The mark grows as the pointer nears, full-size over the dot.
	const topomapProbe: Probe = (px, py, box) => {
		if (!layout || !weights) return null;
		const { cx, cy, side, radius } = headFrame(box.w, box.h);
		if ((px - cx) ** 2 + (py - cy) ** 2 > radius * radius) return null;
		const reach = box.tol * 3;
		let near: (typeof electrodes)[number] | null = null;
		let best = reach * reach;
		for (const e of electrodes) {
			const ex = cx + (e.x - 0.5) * side;
			const ey = cy + (e.y - 0.5) * side;
			const d = (ex - px) ** 2 + (ey - py) ** 2;
			if (d <= best) {
				best = d;
				near = { ...e, x: ex, y: ey };
			}
		}
		if (near) {
			const t = Math.min(1, Math.max(0, (Math.sqrt(best) - DOT) / (reach - DOT)));
			const r = DOT + (6 - DOT) * (1 - t);
			return { mark: { x: near.x, y: near.y, r }, lines: [[formatTick(near.value)], [near.name]] };
		}
		const v = evaluateAt(layout, weights, 0.5 + (px - cx) / side, 0.5 + (py - cy) / side);
		return { mark: null, lines: [[formatTick(v)]] };
	};

	// ── connectivity ─────────────────────────────────────────────────────────────────────────
	/** One chord of a (C, C) frame: the upper triangle, read against the window as `t`. */
	interface Edge {
		a: number;
		b: number;
		value: number;
		t: number;
	}
	/** The edges between the channels drawn, weakest first so the strong ones land on top. */
	function edgesOf(arr: ArrayData, channels: Channel[]): Edge[] {
		const n = arr.shape[0];
		const v = arr.values as ArrayLike<number>;
		const out: Edge[] = [];
		for (let a = 0; a < channels.length; a++) {
			for (let b = a + 1; b < channels.length; b++) {
				const value = Number(v[channels[a].i * n + channels[b].i]);
				if (Number.isFinite(value)) out.push({ a, b, value, t: 0 });
			}
		}
		const [lo, hi] = valueWindow(out.map((e) => e.value));
		const span = hi - lo || 1;
		for (const e of out) e.t = Math.min(1, Math.max(0, (e.value - lo) / span));
		out.sort((x, y) => x.t - y.t);
		return out;
	}
	function rgb(L: Uint8Array, t: number): string {
		const i = ((t * 255) | 0) * 3;
		return `rgb(${L[i]}, ${L[i + 1]}, ${L[i + 2]})`;
	}

	// The points the last paint placed, in layout px, for the hover to name.
	let placed: { name: string; x: number; y: number; i: number }[] = [];
	/** A channel's strongest partner, for the readout. */
	function strongest(edges: Edge[], k: number): Edge | null {
		let best: Edge | null = null;
		for (const e of edges) if ((e.a === k || e.b === k) && (!best || e.t > best.t)) best = e;
		return best;
	}

	let ringEdges: Edge[] = [];
	let ringNames: string[] = [];
	function paintRing(ctx: CanvasRenderingContext2D, arr: ArrayData, channels: Channel[]): void {
		const w = size.w;
		const h = size.h;
		ctx.clearRect(0, 0, w, h);
		if (channels.length < 2) {
			drawMessage(ctx, w, h, 'need ≥ 2 recognized channels');
			placed = [];
			return;
		}
		const edges = edgesOf(arr, channels);
		ringEdges = edges;
		ringNames = channels.map((c) => c.name);
		const L = lutFor(colormap);
		const n = channels.length;
		const cx = w / 2;
		const cy = h / 2;
		// The labels take the outer band; the blocks sit inside it and the chords inside them.
		const label = Math.min(64, Math.max(24, Math.min(w, h) * 0.2));
		const radius = Math.max(8, Math.min(w, h) / 2 - label);
		const block = Math.max(3, Math.min(8, (Math.PI * radius) / n - 1));
		// Channel 0 at the top, then clockwise, as a viewer reads a ring.
		const angle = (k: number) => -Math.PI / 2 + (k / n) * Math.PI * 2;
		placed = channels.map((c, k) => ({
			name: c.name,
			x: cx + Math.cos(angle(k)) * radius,
			y: cy + Math.sin(angle(k)) * radius,
			i: k
		}));
		ctx.lineCap = 'round';
		for (const e of edges) {
			ctx.strokeStyle = rgb(L, e.t);
			ctx.globalAlpha = 0.15 + 0.85 * e.t;
			ctx.lineWidth = 0.5 + 1.5 * e.t;
			ctx.beginPath();
			ctx.moveTo(placed[e.a].x, placed[e.a].y);
			ctx.quadraticCurveTo(cx, cy, placed[e.b].x, placed[e.b].y);
			ctx.stroke();
		}
		ctx.globalAlpha = 1;
		ctx.font = tickFont(10);
		ctx.textBaseline = 'middle';
		for (let k = 0; k < n; k++) {
			const a = angle(k);
			ctx.save();
			ctx.translate(cx, cy);
			ctx.rotate(a);
			ctx.fillStyle = seriesColor(k);
			ctx.fillRect(radius, -block / 2, block * 1.5, block);
			ctx.fillStyle = AXIS_INK;
			// Text reads outward on the right half and is flipped on the left, never upside down.
			const flip = Math.cos(a) < 0;
			if (flip) {
				ctx.rotate(Math.PI);
				ctx.textAlign = 'right';
				ctx.fillText(channels[k].name, -(radius + block * 1.5 + 4), 0);
			} else {
				ctx.textAlign = 'left';
				ctx.fillText(channels[k].name, radius + block * 1.5 + 4, 0);
			}
			ctx.restore();
		}
	}

	const pointProbe: Probe = (px, py, box) => {
		let near: (typeof placed)[number] | null = null;
		let best = box.tol * box.tol;
		for (const p of placed) {
			const d = (p.x - px) ** 2 + (p.y - py) ** 2;
			if (d <= best) {
				best = d;
				near = p;
			}
		}
		if (!near) return null;
		const e = strongest(ringEdges, near.i);
		const lines = [[near.name]];
		if (e) lines.unshift([formatTick(e.value)], [ringNames[e.a === near.i ? e.b : e.a]]);
		return { mark: { x: near.x, y: near.y, r: 4 }, lines };
	};

	// ── 3-D ──────────────────────────────────────────────────────────────────────────────────
	// Turned by the drag and kept while the viewer lives; it is a view, not a setting.
	let camera: Camera = { yaw: 0, pitch: 0.6 };
	const rotate: Drag = (dx, dy) => {
		camera = {
			yaw: camera.yaw + dx * 0.01,
			pitch: Math.max(-Math.PI / 2, Math.min(Math.PI / 2, camera.pitch + dy * 0.01))
		};
		repaint();
	};

	function paint3d(ctx: CanvasRenderingContext2D, arr: ArrayData, channels: Channel[]): void {
		const w = size.w;
		const h = size.h;
		ctx.clearRect(0, 0, w, h);
		if (channels.length < 2) {
			drawMessage(ctx, w, h, 'need ≥ 2 recognized channels');
			placed = [];
			return;
		}
		const edges = edgesOf(arr, channels);
		ringEdges = edges;
		ringNames = channels.map((c) => c.name);
		const L = lutFor(colormap);
		const view = project(camera, w, h);
		// The head: its equator, and the two great circles through the vertex, as the scaffold
		// the electrodes hang on. The nose points anterior.
		ctx.strokeStyle = 'rgba(160, 168, 184, 0.25)';
		ctx.lineWidth = 1;
		const circles: [number, number, number][][] = [[], [], []];
		for (let k = 0; k <= 64; k++) {
			const a = (k / 64) * Math.PI * 2;
			circles[0].push([Math.cos(a), Math.sin(a), 0]);
			circles[1].push([0, Math.cos(a), Math.sin(a)]);
			circles[2].push([Math.cos(a), 0, Math.sin(a)]);
		}
		for (const c of circles) {
			ctx.beginPath();
			c.forEach((p, k) => {
				const q = view(p);
				if (k === 0) ctx.moveTo(q.x, q.y);
				else ctx.lineTo(q.x, q.y);
			});
			ctx.stroke();
		}
		const nose = [view([0, 1, 0]), view([0, 1.15, 0])];
		ctx.strokeStyle = 'rgba(197, 200, 214, 0.7)';
		ctx.beginPath();
		ctx.moveTo(nose[0].x, nose[0].y);
		ctx.lineTo(nose[1].x, nose[1].y);
		ctx.stroke();
		const points = channels.map((c) => view(lift(c.pos)));
		placed = channels.map((c, k) => ({ name: c.name, x: points[k].x, y: points[k].y, i: k }));
		ctx.lineCap = 'round';
		for (const e of edges) {
			ctx.strokeStyle = rgb(L, e.t);
			ctx.globalAlpha = 0.15 + 0.85 * e.t;
			ctx.lineWidth = 0.5 + 1.5 * e.t;
			ctx.beginPath();
			ctx.moveTo(points[e.a].x, points[e.a].y);
			ctx.lineTo(points[e.b].x, points[e.b].y);
			ctx.stroke();
		}
		ctx.globalAlpha = 1;
		// The far side of the head is drawn smaller and dimmer, which is all the depth a dot needs.
		for (const p of points) {
			ctx.fillStyle = `rgba(230, 233, 240, ${0.35 + 0.65 * p.depth})`;
			ctx.beginPath();
			ctx.arc(p.x, p.y, 2 + 2 * p.depth, 0, Math.PI * 2);
			ctx.fill();
		}
	}

	// ── dispatch ─────────────────────────────────────────────────────────────────────────────
	function repaint(): void {
		if (!frame || !canvas) return;
		const ctx = canvas.getContext('2d');
		if (!ctx) return;
		const arr = frame.data as ArrayData;
		const names = ((frame.meta?.channels as { dim0?: string[] }) ?? {}).dim0 ?? [];
		const dpr = window.devicePixelRatio || 1;
		const bw = Math.max(1, Math.round(size.w * dpr));
		const bh = Math.max(1, Math.round(size.h * dpr));
		if (canvas.width !== bw) canvas.width = bw;
		if (canvas.height !== bh) canvas.height = bh;
		ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
		const channels = known(names);
		if (mode === 'topomap') paintTopomap(ctx, arr, channels);
		else if (mode === 'ring') paintRing(ctx, arr, channels);
		else paint3d(ctx, arr, channels);
	}

	$effect(() => {
		// Repaint on a new frame, a new size, or any colormap / range / contour / mode change.
		void [colormap, autoRange, vmin, vmax, contours, mode, size, frame];
		repaint();
	});
	$effect(() => {
		probe = mode === 'topomap' ? topomapProbe : pointProbe;
		drag = mode === '3d' ? rotate : null;
	});

	$effect(() => {
		const c = canvas;
		if (!c) return;
		const updateSize = () => (size = { w: c.clientWidth, h: c.clientHeight });
		updateSize();
		resizer = new ResizeObserver(updateSize);
		resizer.observe(c);
		return () => resizer?.disconnect();
	});
	onDestroy(() => resizer?.disconnect());
</script>

<canvas bind:this={canvas}></canvas>

<style>
	canvas {
		width: 100%;
		height: 100%;
		min-height: 100px;
		min-width: 100px;
		display: block;
	}
</style>
