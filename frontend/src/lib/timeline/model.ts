/* The timeline's arithmetic: what the panel draws its tracks by, apart from the drawing. */
import type { Lane, Note, Track } from '$lib/api/generated';
import { curveAt } from '$lib/ui/curve';

/** The seconds one time unit lasts: a beat of the tempo, or a second. */
export const unit = (beats: boolean, bpm: number): number => (beats ? 60 / Math.max(bpm, 1e-9) : 1);

/** Where the last note or keyframe of every track ends, in seconds. */
export function contentEnd(tracks: Track[], bpm: number): number {
	const ends = tracks.flatMap((t) => {
		const ats = t.kind === 'note' ? t.notes.map((n) => n.at + n.length) : Object.values(t.attributes).flatMap((l) => l.keys.map((k) => k.at));
		return ats.map((at) => at * unit(t.beats, bpm));
	});
	return Math.max(0, ...ends);
}

/** The ruler's marks over `span` units: at most `want` gaps, at a round step. */
export function ticks(span: number, want = 8): number[] {
	const raw = span / want;
	const mag = 10 ** Math.floor(Math.log10(raw));
	const step = [1, 2, 5, 10].map((m) => m * mag).find((s) => s >= raw) ?? raw;
	return Array.from({ length: Math.floor(span / step + 1e-9) + 1 }, (_, i) => Number((i * step).toPrecision(6)));
}

/** The pitches a note lane shows, lowest and highest: two octaves from C3, widened to hold every note. */
export function pitches(notes: Note[]): [number, number] {
	return [Math.min(48, ...notes.map((n) => n.pitch - 2)), Math.max(72, ...notes.map((n) => n.pitch + 2))];
}

/** A lane's curve as an SVG path over a `w` by `h` box that spans `span` of the track's time
 * units: each keyframe eases to the next by its curve, and the value holds outside them. */
export function lanePath(lane: Lane, span: number, w: number, h: number, steps = 16): string {
	const [lo, hi] = lane.range;
	const at = (t: number, v: number): string => `${Number(((w * t) / span).toFixed(2))} ${Number((h * (1 - (v - lo) / (hi - lo || 1))).toFixed(2))}`;
	const keys = lane.keys;
	if (keys.length === 0) return '';
	const points = [`M${at(0, keys[0].value)}`];
	keys.forEach((k, i) => {
		const next = keys[i + 1];
		for (let s = 0; s < (next ? steps : 1); s++) {
			const f = s / steps;
			points.push(`L${at(k.at + ((next?.at ?? k.at) - k.at) * f, k.value + ((next?.value ?? k.value) - k.value) * curveAt(k.curve, f))}`);
		}
	});
	return `${points.join(' ')} L${at(span, keys[keys.length - 1].value)}`;
}

/** The tracks a menu entry hides, of those a panel shows: the track, those above it, those below
 * it, or every other. */
export function hiding(shown: string[], id: string, which: 'this' | 'above' | 'below' | 'others'): string[] {
	const at = shown.indexOf(id);
	return which === 'this' ? [id] : which === 'above' ? shown.slice(0, at) : which === 'below' ? shown.slice(at + 1) : shown.filter((t) => t !== id);
}
