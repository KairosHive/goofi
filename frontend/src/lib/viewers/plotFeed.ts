/** What an array frame becomes on its way to a plot: the reductions the viewers did on their own canvases. */
import type { ArrayData, DataFrame } from '$lib/codec/decode';
import { extent, type ImagePlot, type LineData } from 'plotluck';
import { decimateMinMax } from './decimate';
import { sampleRange } from './depth';
import type { SettingsMap } from './module';

/** `cols` is the plot's width in device px: a frame denser than two samples per column is min/max folded. */
export function lineData(frame: DataFrame, cols: number, logX: boolean): LineData {
	const arr = frame.data as ArrayData;
	const v = arr.values;
	if (v.length === 1) return { rows: [v] };
	const shape = arr.shape;
	const series = shape.length <= 1 ? 1 : shape[0];
	const m = shape.length <= 1 ? v.length : shape[1];
	const rows: ArrayLike<number>[] = [];
	for (let c = 0; c < series; c++) rows.push(v.subarray(c * m, (c + 1) * m));
	// The index axis starts at 1 under log x, since log10(0) has no place on it.
	const base = logX ? 1 : 0;
	if (m > cols * 2) {
		const dec = decimateMinMax(rows, m, cols, base);
		return { rows: dec.ys, xs: dec.xs };
	}
	return { rows, base };
}

/** Only the wire's f32 and the reducer's 8-bit hop are textures; nothing else reaches an image viewer.
 * One and two channels share one window: the LUT input, or red and green alike. */
export function pushImage(plot: ImagePlot, frame: DataFrame, settings: SettingsMap): void {
	const arr = frame.data as ArrayData;
	const values = arr.values;
	const [height, width] = arr.shape;
	const channels = arr.shape.length === 3 ? arr.shape[2] : 1;
	// An 8-bit texel samples in [0, 1] whatever the frame's units, so a manual range is scaled with it.
	const range = sampleRange(arr.dtype, frame.meta);
	const [t0, t1] = range ?? [0, 1];
	const tw = t1 - t0 || 1;
	let lo = 0;
	let hi = 1;
	if (channels <= 2 && settings.auto === false) {
		lo = (Number(settings.vmin ?? 0) - t0) / tw;
		hi = (Number(settings.vmax ?? 1) - t0) / tw;
	} else if (channels <= 2 && !range) {
		[lo, hi] = extent(values) ?? [0, 1];
	}
	plot.push({ values: channels === 4 ? overChecker(values, width, height) : values, width, height, channels, lo, hi });
}

/** How many texels a square of the checkerboard spans. */
const CHECK = 8;

/** An RGBA frame composited over the classic grey checkerboard, so its alpha reads as what it is
 * on a surface that is drawn opaque. The caller's array is never written. */
export function overChecker(values: Uint8Array | Float32Array, width: number, height: number): Uint8Array | Float32Array {
	const u8 = values instanceof Uint8Array;
	const out = u8 ? new Uint8Array(values.length) : new Float32Array(values.length);
	const full = u8 ? 255 : 1;
	for (let y = 0; y < height; y++) {
		for (let x = 0; x < width; x++) {
			const i = (y * width + x) * 4;
			const check = (((x / CHECK) | 0) + ((y / CHECK) | 0)) % 2 === 0 ? 0.5 * full : 0.75 * full;
			const a = Math.min(1, Math.max(0, values[i + 3] / full));
			for (let c = 0; c < 3; c++) out[i + c] = values[i + c] * a + check * (1 - a);
			out[i + 3] = full;
		}
	}
	return out;
}
