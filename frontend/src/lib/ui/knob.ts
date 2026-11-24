/** `step`, or by default about 200 stops across the span. */
export function stepOf(min: number, max: number, step?: number): number {
	return step && step > 0 ? step : Math.max((max - min) / 200, 1e-6);
}

/** `n` on the step's decimal grid: float arithmetic leaves a tail that the step's digits remove. */
export function onStep(n: number, step: number): number {
	const places = (String(step).split('.')[1] ?? '').length;
	return places ? Number(n.toFixed(places)) : n;
}

/** What a vertical drag of `dy` pixels does to a value in `[min, max]`, on `step`. A full sweep is
 * `span` pixels, so a taller drag is not a wilder jump — the range decides, not the screen. */
export function turnedBy(
	value: number,
	dy: number,
	min: number,
	max: number,
	step: number,
	span = 160
): number {
	const next = value - (dy / span) * (max - min);
	const stepped = step > 0 ? Math.round(next / step) * step : next;
	return onStep(Math.min(max, Math.max(min, stepped)), step);
}
