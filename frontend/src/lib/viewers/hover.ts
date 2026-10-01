/** What a viewer says about the point under the pointer: each kind defines its own probe, and
 * the feed draws the answer. Positions are in the body's layout px. */
import type { ArrayData } from '$lib/codec/decode';
import type { LineData, Range } from 'plotluck';
import { sampleRange, toUnit } from './depth';
import { formatTick } from './format';

export interface Hover {
	/** The point the readout is about, marked with a dot of radius `r`; the readout follows the pointer. */
	mark: { x: number; y: number; r: number } | null;
	/** The value on top, then where it is: each line's parts sit in one row. */
	lines: string[][];
}

export interface ProbeBox {
	w: number;
	h: number;
	/** How far, in layout px, the pointer may sit from a point and still read it. */
	tol: number;
}

export type Probe = (x: number, y: number, box: ProbeBox) => Hover | null;

/** What a viewer does with a drag of `dx`, `dy` layout px. A viewer that defines one captures
 * the pointer; one that does not leaves the drag to whatever holds it, such as the node card. */
export type Drag = (dx: number, dy: number, box: ProbeBox) => void;

/** The names a frame's meta gives axis `dim`, as strings whatever a producer sent, or `null`
 * where it names none. */
export function axisNames(meta: Record<string, unknown> | undefined, dim: number): string[] | null {
	const channels = meta?.channels as Record<string, unknown> | undefined;
	const names = channels?.[`dim${dim}`];
	return Array.isArray(names) ? names.map(String) : null;
}

/** A coordinate as its axis name when the axis has one, else the integer or the tick format. A
 * name that is itself a number is shown to the tick format's precision, not to its full length. */
function coord(v: number, names: string[] | null): string {
	if (names && Number.isInteger(v) && v >= 0 && v < names.length) {
		const name = names[v];
		return name.trim() !== '' && Number.isFinite(Number(name)) ? formatTick(Number(name)) : name;
	}
	return Number.isInteger(v) ? String(v) : formatTick(v);
}

function log(v: number, on: boolean): number {
	return on ? Math.log10(v) : v;
}

export interface LineAxes {
	logX: boolean;
	logY: boolean;
	/** The plot's inner margin in layout px; plotluck keeps a 2-device-px pad around the range. */
	pad: number;
}

export interface LineNames {
	x: string[] | null;
	series: string[] | null;
}

/** The drawn vertex nearest the pointer, within `tol`; `data.xs` must not decrease. */
export function lineProbe(data: LineData, range: Range, axes: LineAxes, names: LineNames): Probe {
	if (range.scalar) return () => null;
	const rows = data.rows;
	const m = rows[0]?.length ?? 0;
	const base = data.base ?? 0;
	// One row of positions shared by every series; a trajectory's per-row xs take `trajectoryProbe`.
	const xs = data.xs as ArrayLike<number> | null | undefined;
	const xAt = (i: number) => (xs ? xs[i] : i + base);
	const x0 = log(range.xMin, axes.logX);
	const x1 = log(range.xMax, axes.logX);
	const y0 = log(range.yMin, axes.logY);
	const y1 = log(range.yMax, axes.logY);
	return (px, py, box) => {
		const w = box.w - 2 * axes.pad;
		const h = box.h - 2 * axes.pad;
		if (m === 0 || w <= 0 || h <= 0) return null;
		const toPx = (x: number) => axes.pad + ((log(x, axes.logX) - x0) / (x1 - x0 || 1)) * w;
		const toPy = (y: number) => axes.pad + (1 - (log(y, axes.logY) - y0) / (y1 - y0 || 1)) * h;
		// The first vertex at or right of the pointer, then outward while the x distance allows.
		let lo = 0;
		let hi = m;
		while (lo < hi) {
			const mid = (lo + hi) >> 1;
			if (toPx(xAt(mid)) < px) lo = mid + 1;
			else hi = mid;
		}
		let best: { i: number; s: number; d: number } | null = null;
		const consider = (i: number): boolean => {
			const dx = toPx(xAt(i)) - px;
			if (Math.abs(dx) > box.tol) return false;
			for (let s = 0; s < rows.length; s++) {
				const y = Number(rows[s][i]);
				if (!Number.isFinite(y) || (axes.logY && y <= 0)) continue;
				const dy = toPy(y) - py;
				const d = dx * dx + dy * dy;
				if (d <= box.tol * box.tol && (!best || d < best.d)) best = { i, s, d };
			}
			return true;
		};
		for (let i = lo; i < m && consider(i); i++) continue;
		for (let i = lo - 1; i >= 0 && consider(i); i--) continue;
		if (!best) return null;
		const { i, s } = best;
		const x = xAt(i);
		const y = Number(rows[s][i]);
		const lines = [[formatTick(y)], [`x ${coord(x, names.x)}`]];
		if (rows.length > 1) lines.unshift([coord(s, names.series)]);
		return { mark: { x: toPx(x), y: toPy(y), r: 3.5 }, lines };
	};
}

/** The drawn point nearest the pointer on a trajectory, within `tol`: every path is scanned,
 * since each has x positions of its own. `pairs` names the rows a path was drawn from. */
export function trajectoryProbe(
	data: LineData,
	range: Range,
	pad: number,
	pairs: [number, number][],
	names: string[] | null
): Probe {
	const rows = data.rows;
	const xs = data.xs as ArrayLike<number>[] | undefined;
	const m = rows[0]?.length ?? 0;
	return (px, py, box) => {
		const w = box.w - 2 * pad;
		const h = box.h - 2 * pad;
		if (!xs || m === 0 || w <= 0 || h <= 0) return null;
		const toPx = (x: number) => pad + ((x - range.xMin) / (range.xMax - range.xMin || 1)) * w;
		const toPy = (y: number) => pad + (1 - (y - range.yMin) / (range.yMax - range.yMin || 1)) * h;
		let best: { i: number; s: number; d: number } | null = null;
		for (let s = 0; s < rows.length; s++) {
			for (let i = 0; i < m; i++) {
				const x = Number(xs[s][i]);
				const y = Number(rows[s][i]);
				if (!Number.isFinite(x) || !Number.isFinite(y)) continue;
				const dx = toPx(x) - px;
				const dy = toPy(y) - py;
				const d = dx * dx + dy * dy;
				if (d <= box.tol * box.tol && (!best || d < best.d)) best = { i, s, d };
			}
		}
		if (!best) return null;
		const { i, s } = best;
		const x = Number(xs[s][i]);
		const y = Number(rows[s][i]);
		const [a, b] = pairs[s];
		return {
			mark: { x: toPx(x), y: toPy(y), r: 3.5 },
			lines: [[formatTick(x), formatTick(y)], [`${coord(a, names)} · ${coord(b, names)}`, `t ${i}`]]
		};
	};
}

/** The image cell under the pointer, with its value per channel; nothing off the image. */
export function imageProbe(arr: ArrayData, stretch: boolean, meta: Record<string, unknown>): Probe {
	const [ih, iw] = arr.shape;
	const channels = arr.shape.length === 3 ? arr.shape[2] : 1;
	const rowNames = axisNames(meta, 0);
	const colNames = axisNames(meta, 1);
	// 8-bit texels read back in the node's own units, as the drawing maps them.
	const range = sampleRange(arr.dtype, meta);
	const value = (t: number): number => (range ? toUnit(t, range) : t);
	return (px, py, box) => {
		if (iw <= 0 || ih <= 0) return null;
		// Centred at the image's own aspect unless stretched, as the surface fits it.
		let fx = 0;
		let fy = 0;
		let fw = box.w;
		let fh = box.h;
		if (!stretch) {
			const scale = Math.min(box.w / iw, box.h / ih);
			fw = iw * scale;
			fh = ih * scale;
			fx = (box.w - fw) / 2;
			fy = (box.h - fh) / 2;
		}
		const col = Math.floor(((px - fx) / fw) * iw);
		const row = Math.floor(((py - fy) / fh) * ih);
		if (col < 0 || col >= iw || row < 0 || row >= ih) return null;
		const at = (row * iw + col) * channels;
		const values: string[] = [];
		for (let c = 0; c < channels; c++) values.push(formatTick(value(Number(arr.values[at + c]))));
		return { mark: null, lines: [values, [`x ${coord(col, colNames)}`, `y ${coord(row, rowNames)}`]] };
	};
}
