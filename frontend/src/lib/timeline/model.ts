/* The timeline's arithmetic: what the panel draws its tracks by, apart from the drawing. */
import type { Key, Track } from '$lib/api/generated';
import { curveAt } from '$lib/ui/curve';

/** The seconds one time unit lasts: a beat of the tempo, or a second. */
export const unit = (beats: boolean, bpm: number): number => (beats ? 60 / Math.max(bpm, 1e-9) : 1);

/** Where the last note or keyframe of every track ends, in seconds. */
export function contentEnd(tracks: Track[], bpm: number): number {
	const ends = tracks.flatMap((t) => (t.kind === 'note' ? t.notes.map((n) => n.at + n.length) : t.keys.map((k) => k.at)).map((at) => at * unit(t.beats, bpm)));
	return Math.max(0, ...ends);
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

/** The lines between a lane's cells, `cell` seconds apart, over the seconds in view, as a path in
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
