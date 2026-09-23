/**
 * The window a log scale may be given: positive and finite, or `log10` makes the scale NaN. A PSD
 * reaches zero, so this is the ordinary case.
 */
export function logSafe(min: number, max: number): [number, number] {
	const hi = Number.isFinite(max) && max > 0 ? max : 1;
	const lo = Number.isFinite(min) && min > 0 && min < hi ? min : hi / 1e3;
	return [lo, hi];
}
