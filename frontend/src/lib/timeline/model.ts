/* The timeline's arithmetic: what the panel draws its tracks by, apart from the drawing. */
import type { Key, Track } from '$lib/api/generated';
import { curveAt } from '$lib/ui/curve';

/** The seconds a beat of the tempo lasts; a tempo of nothing has the longest beat there is. */
export const beatSeconds = (bpm: number): number => 60 / Math.max(bpm, 1e-9);

/** Where the last note or keyframe of every track ends, in beats. */
export function contentEnd(tracks: Track[]): number {
	return Math.max(0, ...tracks.flatMap((t) => (t.kind === 'note' ? t.notes.map((n) => n.at + n.length) : t.keys.map((k) => k.at))));
}

/** The ruler's marks between `from` and `to`: at most `want` gaps, at a round step. */
export function ticks(from: number, to: number, want = 8): number[] {
	const raw = (to - from) / want;
	const mag = 10 ** Math.floor(Math.log10(raw));
	const step = [1, 2, 5, 10].map((m) => m * mag).find((s) => s >= raw)!;
	const first = Math.ceil(from / step - 1e-9);
	// A mark is a whole count of steps, spelled to the step's own decimals.
	const decimals = Math.max(0, -Math.floor(Math.log10(step)));
	return Array.from({ length: Math.max(0, Math.floor(to / step + 1e-9) - first + 1) }, (_, i) => Number(((first + i) * step).toFixed(decimals)));
}

/** A curve as an SVG path in a box 1000 along the time axis and 100 across it: each keyframe eases
 * to the next by its curve, and the value holds outside them. `along` is where a time falls on
 * the axis, 0 to 1 over what is in view; `flip` runs the time axis down, the value to the right. */
export function lanePath(keys: Key[], [lo, hi]: [number, number], along: (at: number) => number, flip = false, steps = 16): string {
	if (keys.length === 0) return '';
	const at = (f: number, v: number): string => {
		const [a, c] = [Number((1000 * f).toFixed(1)), Number((100 * ((v - lo) / (hi - lo || 1))).toFixed(1))];
		return flip ? `${c} ${a}` : `${a} ${100 - c}`;
	};
	const points = [`M${at(Math.min(0, along(keys[0].at)), keys[0].value)}`];
	keys.forEach((k, i) => {
		const next = keys[i + 1];
		for (let s = 0; s < (next ? steps : 1); s++) {
			const f = s / steps;
			points.push(`L${at(along(k.at + ((next?.at ?? k.at) - k.at) * f), k.value + ((next?.value ?? k.value) - k.value) * curveAt(k.curve, f))}`);
		}
	});
	const last = keys[keys.length - 1];
	return `${points.join(' ')} L${at(Math.max(1, along(last.at)), last.value)}`;
}

/** The lines between a lane's cells, `cell` beats apart, over the beats in view, as a path in
 * the box `lanePath` draws in. Cells too many to tell apart are drawn by the two, the four, and on. */
export function gridPath(from: number, to: number, cell: number, flip = false): string {
	let step = cell;
	while ((to - from) / step > 200) step *= 2;
	const first = Math.ceil(from / step - 1e-9);
	return Array.from({ length: Math.max(0, Math.floor(to / step + 1e-9) - first + 1) }, (_, i) => {
		const a = Number(((1000 * ((first + i) * step - from)) / (to - from)).toFixed(1));
		return flip ? `M0 ${a}H100` : `M${a} 0V100`;
	}).join('');
}

/** The tracks a menu entry hides, of those a panel shows: the track, those above it, those below
 * it, or every other. */
export function hiding(shown: string[], id: string, which: 'this' | 'above' | 'below' | 'others'): string[] {
	const at = shown.indexOf(id);
	return which === 'this' ? [id] : which === 'above' ? shown.slice(0, at) : which === 'below' ? shown.slice(at + 1) : shown.filter((t) => t !== id);
}

/** The most of a range bar that a view past an end of the axis folds into. */
const FOLD = 0.15;
/** Where a point of an axis from 0 to `span` stands on a bar of the axis's length, 0 the bar's
 * start and 1 its end. Past an end it is that far past at first, then less and less, and never
 * as far as `FOLD`: a view with no widest folds into the bar's end. */
export function barPlace(at: number, span: number): number {
	const fold = (past: number): number => (FOLD * past) / (past + FOLD);
	return at < 0 ? -fold(-at / span) : at > span ? 1 + fold(at / span - 1) : at / span;
}
/** The point of the axis that stands at a place on the bar: `barPlace` the other way round. A
 * place past where a fold ends is the furthest point a fold holds. */
export function barPoint(on: number, span: number): number {
	const unfold = (by: number): number => (FOLD * Math.min(by, FOLD * 0.999)) / (FOLD - Math.min(by, FOLD * 0.999));
	return span * (on < 0 ? -unfold(-on) : on > 1 ? 1 + unfold(on - 1) : on);
}
