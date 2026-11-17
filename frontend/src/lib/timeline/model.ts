/* The timeline's arithmetic: what the panel draws its tracks by, apart from the drawing. */
import type { Key, Note, Track } from '$lib/api/generated';
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
	return Array.from({ length: Math.max(0, Math.floor(to / step + 1e-9) - first + 1) }, (_, i) => Number(((first + i) * step).toPrecision(6)));
}

/** The pitches a note lane shows, lowest and highest: two octaves from C3, widened to hold every note. */
export function pitches(notes: Note[]): [number, number] {
	return [Math.max(0, Math.min(48, ...notes.map((n) => n.pitch - 2))), Math.min(127, Math.max(72, ...notes.map((n) => n.pitch + 2)))];
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

/** The tracks a menu entry hides, of those a panel shows: the track, those above it, those below
 * it, or every other. */
export function hiding(shown: string[], id: string, which: 'this' | 'above' | 'below' | 'others'): string[] {
	const at = shown.indexOf(id);
	return which === 'this' ? [id] : which === 'above' ? shown.slice(0, at) : which === 'below' ? shown.slice(at + 1) : shown.filter((t) => t !== id);
}
