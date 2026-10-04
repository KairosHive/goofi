// A hand stroke in the text form `control paint` takes; `goofi_core::drawing` owns the format and
// rasterizes it onto the pad's array. Coordinates, width and softness span `SPAN`.

export const SPAN = 1000;

export type Ink = string | 'erase';

/** A hand stroke in the text form `control paint` takes. Points are `[x, y, ms since the last]`. */
export function strokeText(
	ink: Ink,
	width: number,
	soft: number,
	points: [number, number, number][],
	dt: number
): string {
	const n = (v: number) => String(Math.round(Math.min(Math.max(v, 0), SPAN) * 10) / 10);
	const t = (ms: number) => (ms > 0 ? `+${Math.round(ms)} ` : '');
	const segs = points.map(([x, y, ms], i) => `${t(ms)}${i ? 'L' : 'M'} ${n(x)} ${n(y)}`);
	// A tap is a dot: a zero-length line with round caps still leaves a mark.
	if (points.length === 1) segs.push(`L ${n(points[0][0])} ${n(points[0][1])}`);
	return `${t(dt)}stroke ${ink} width ${n(width)} soft ${n(soft)} : ${segs.join(' ')}`;
}
