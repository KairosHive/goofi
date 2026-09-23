/** What an array frame becomes on its way to a plot: the reductions the viewers did on their own canvases. */
import type { ArrayData, DataFrame } from '$lib/codec/decode';
import { extent, type ImagePlot, type LinePlot } from 'glance';
import { decimateMinMax } from './decimate';
import { isU8, sampleRange } from './depth';
import { envelopeBand, readEnvelope } from './envelope';
import type { SettingsMap } from './settingsSchema';

/** `cols` is the plot's width in device px: a frame denser than two samples per column is min/max folded. */
export function pushLine(plot: LinePlot, frame: DataFrame, cols: number, logX: boolean): void {
	const arr = frame.data as ArrayData;
	const v = arr.values;
	if (v.length === 1) {
		plot.push({ rows: [v] });
		return;
	}
	const shape = arr.shape;
	const series = shape.length <= 1 ? 1 : shape[0];
	const m = shape.length <= 1 ? v.length : shape[1];
	const rows: ArrayLike<number>[] = [];
	for (let c = 0; c < series; c++) rows.push(v.subarray(c * m, (c + 1) * m));
	// The index axis starts at 1 under log x, since log10(0) has no place on it.
	const base = logX ? 1 : 0;
	const env = readEnvelope(frame.meta, shape.length);
	if (env) {
		const band = envelopeBand(rows, env.origLen, base);
		plot.push({ rows: band.ys, xs: band.xs });
	} else if (m > cols * 2) {
		const dec = decimateMinMax(rows, m, cols, base);
		plot.push({ rows: dec.ys, xs: dec.xs });
	} else {
		plot.push({ rows, base });
	}
}

/** Only the wire's f32 and the reducer's 8-bit hop are textures; nothing else reaches an image viewer.
 * One and two channels share one window: the LUT input, or red and green alike. */
export function pushImage(plot: ImagePlot, frame: DataFrame, settings: SettingsMap): void {
	const arr = frame.data as ArrayData;
	const values = arr.values;
	const [height, width] = arr.shape;
	const channels = arr.shape.length === 3 ? arr.shape[2] : 1;
	const u8 = isU8(arr.dtype);
	// An 8-bit texel samples in [0, 1] whatever the frame's units, so a manual range is scaled with it.
	const texels = (u8 && sampleRange(frame.meta)) || [0, 1];
	const t0 = u8 ? texels[0] : 0;
	const tw = u8 ? texels[1] - texels[0] || 1 : 1;
	let lo = 0;
	let hi = 1;
	if (channels <= 2 && settings.auto === false) {
		lo = (Number(settings.vmin ?? 0) - t0) / tw;
		hi = (Number(settings.vmax ?? 1) - t0) / tw;
	} else if (channels <= 2 && !u8) {
		[lo, hi] = extent(values) ?? [0, 1];
	}
	plot.push({ values, width, height, channels, lo, hi });
}
