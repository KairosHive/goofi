/** The pure half of plotluck: rectangles, windows, grids and buffer layout. No GL here. */

/** A plot's box in the host's world units. */
export interface Rect {
	x: number;
	y: number;
	w: number;
	h: number;
}

/** The pane in CSS pixels, the camera in world units, and the device pixel ratio. */
export interface View {
	x: number;
	y: number;
	zoom: number;
	width: number;
	height: number;
	dpr: number;
}

/** Whole device pixels, y down from the top-left of the pane, so scissor and lines agree. */
export function deviceRect(r: Rect, v: View): Rect {
	const x0 = Math.round((r.x * v.zoom + v.x) * v.dpr);
	const y0 = Math.round((r.y * v.zoom + v.y) * v.dpr);
	const x1 = Math.round(((r.x + r.w) * v.zoom + v.x) * v.dpr);
	const y1 = Math.round(((r.y + r.h) * v.zoom + v.y) * v.dpr);
	return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}

/** Whether a device rect meets the pane at all. */
export function visible(d: Rect, v: View): boolean {
	return (
		d.w > 0 &&
		d.h > 0 &&
		d.x < v.width * v.dpr &&
		d.y < v.height * v.dpr &&
		d.x + d.w > 0 &&
		d.y + d.h > 0
	);
}

/** The window a log scale may be given: positive and finite, or log10 makes the scale NaN. */
export function logSafe(min: number, max: number): [number, number] {
	const hi = Number.isFinite(max) && max > 0 ? max : 1;
	const lo = Number.isFinite(min) && min > 0 && min < hi ? min : hi / 1e3;
	return [lo, hi];
}

/** The smallest normal float32: below it a value is underflow residue, as good as zero to a log. */
const F32_NORMAL_MIN = 2 ** -126;

/** The finite extent of every `stride`-th value from `offset`, on request only values a log axis
 * can show (at least the smallest normal float32); `null` when no value qualifies. */
export function extent(
	values: ArrayLike<number>,
	offset = 0,
	stride = 1,
	positive = false
): [number, number] | null {
	let lo = Infinity;
	let hi = -Infinity;
	for (let i = offset; i < values.length; i += stride) {
		const v = values[i];
		if (!Number.isFinite(v) || (positive && v < F32_NORMAL_MIN)) continue;
		if (v < lo) lo = v;
		if (v > hi) hi = v;
	}
	return lo <= hi ? [lo, hi] : null;
}

/** A data value in axis space: log10 when the axis is logarithmic. */
export function mapped(v: number, log: boolean): number {
	return log ? Math.log10(v) : v;
}

export function unmapped(v: number, log: boolean): number {
	return log ? 10 ** v : v;
}

/** The axis window for `[lo, hi]` in axis space: log-floored, padded by `pad` of its span each
 * side so a line on the bound is not cut in half. A flat span opens to ±1. */
export function axisWindow(lo: number, hi: number, log: boolean, pad = 0.05): [number, number] {
	if (log) [lo, hi] = logSafe(lo, hi);
	if (lo > hi) [lo, hi] = [hi, lo];
	let a = mapped(lo, log);
	let b = mapped(hi, log);
	if (!(a < b)) return [a - 1, b + 1];
	const p = (b - a) * pad;
	return [a - p, b + p];
}

/** Grid positions in `[a, b]` as fractions of the span: 1-2-5 steps near `target` lines on a
 * linear axis, decades with minor lines at the mantissas 2..9 on a log axis. */
export function gridLines(a: number, b: number, log: boolean, target = 4): { major: number[]; minor: number[] } {
	const major: number[] = [];
	const minor: number[] = [];
	if (!(b > a) || !Number.isFinite(a) || !Number.isFinite(b)) return { major, minor };
	const span = b - a;
	const add = (out: number[], v: number): void => {
		const t = (v - a) / span;
		if (t > 1e-3 && t < 1 - 1e-3) out.push(t);
	};
	if (log) {
		for (let k = Math.ceil(a); k <= Math.floor(b); k++) add(major, k);
		// Past a handful of decades the mantissa lines crowd; the decades alone say enough.
		if (span <= 6) {
			for (let k = Math.floor(a); k <= Math.floor(b); k++) for (let d = 2; d <= 9; d++) add(minor, k + Math.log10(d));
		}
		return { major, minor };
	}
	const raw = span / target;
	const mag = 10 ** Math.floor(Math.log10(raw));
	const unit = raw / mag;
	const step = (unit < 1.5 ? 1 : unit < 3.5 ? 2 : unit < 7.5 ? 5 : 10) * mag;
	for (let v = Math.ceil(a / step) * step; v <= b; v += step) add(major, v);
	return { major, minor };
}

/** Sample positions: one row shared by every series, or one row per series. */
export type Xs = ArrayLike<number> | ArrayLike<number>[];

/** Whether `xs` gives each series positions of its own. */
export function perRow(xs: Xs | null | undefined): xs is ArrayLike<number>[] {
	return !!xs && xs.length > 0 && typeof (xs as ArrayLike<unknown>)[0] === 'object';
}

/** Interleave rows into (x, y) pairs with a NaN pad after each row, so one instanced segment
 * draw covers every series and never joins two rows. Reuses `out` when it fits. */
export function layoutSeries(
	rows: ArrayLike<number>[],
	xs: Xs | null,
	base: number,
	out: Float32Array | null
): { buf: Float32Array; stride: number; m: number } {
	const m = rows.length ? rows[0].length : 0;
	const stride = m + 1;
	// A NaN pad before the first row and two after the last, so every segment has a neighbour
	// to read on each side: a joint where the neighbour is a point, a butt end where it is a pad.
	const n = (rows.length * stride + 2) * 2;
	const buf = out && out.length === n ? out : new Float32Array(n);
	buf[0] = NaN;
	buf[1] = NaN;
	const rowXs = perRow(xs);
	for (let s = 0; s < rows.length; s++) {
		const row = rows[s];
		const x = rowXs ? xs[s] : (xs as ArrayLike<number> | null);
		let o = (1 + s * stride) * 2;
		for (let i = 0; i < m; i++) {
			buf[o++] = x ? x[i] : i + base;
			buf[o++] = row[i];
		}
		buf[o] = NaN;
		buf[o + 1] = NaN;
	}
	buf[n - 2] = NaN;
	buf[n - 1] = NaN;
	return { buf, stride, m };
}

/** Where a `w`×`h` image sits inside a device rect: centred at its own aspect unless stretched. */
export function fitImage(d: Rect, w: number, h: number, stretch: boolean): Rect {
	if (stretch || w <= 0 || h <= 0) return d;
	const scale = Math.min(d.w / w, d.h / h);
	const fw = Math.round(w * scale);
	const fh = Math.round(h * scale);
	return { x: d.x + ((d.w - fw) >> 1), y: d.y + ((d.h - fh) >> 1), w: fw, h: fh };
}

/** `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa` as floats in 0..1. */
export function rgba(hex: string): [number, number, number, number] {
	let h = hex.trim().replace('#', '');
	if (h.length <= 4) h = [...h].map((c) => c + c).join('');
	const n = parseInt(h, 16);
	if (h.length === 8) {
		return [((n >>> 24) & 255) / 255, ((n >>> 16) & 255) / 255, ((n >>> 8) & 255) / 255, (n & 255) / 255];
	}
	return [((n >>> 16) & 255) / 255, ((n >>> 8) & 255) / 255, (n & 255) / 255, 1];
}
