/* The timeline's arithmetic: what the panel draws its tracks by, apart from the drawing. */
import type { Curve, Key, Note, Track } from '$lib/api/generated';
import { NOTE_NAMES, SCALES } from '$lib/api/vocab';
import { curveAt } from '$lib/ui/curve';

/** How a panel draws a note track's keyboard, its own and no part of the track: the pitches from
 * its lowest to its highest, the scale and the root it marks and a note lands on, and the grid a
 * note snaps to. */
export interface Keyboard {
	range: [number, number];
	scale: string;
	root: string;
	grid: string;
}
/** The grids a note snaps to, by name: how many seconds or beats lie between two lines, and
 * whether they are beats. */
export const GRIDS: Record<string, readonly [number, boolean]> = {
	'1 s': [1, false],
	'1/2 s': [0.5, false],
	'1/4 s': [0.25, false],
	'1/10 s': [0.1, false],
	'1 beat': [1, true],
	'1/2 beat': [0.5, true],
	'1/3 beat': [1 / 3, true],
	'1/4 beat': [0.25, true],
	'1/8 beat': [0.125, true],
	'1/16 beat': [0.0625, true]
};
/** The keyboard for `notes` with what a panel `set` of it: two octaves from C3 and every note the
 * track holds, chromatic from C, on quarter beats, but for what is set. What is set and is no
 * keyboard's, as a file or an agent may put in a panel's state, is as if unset. */
export function keyboard(notes: Note[], set: Partial<Keyboard> = {}): Keyboard {
	const pitches = notes.map((n) => n.pitch);
	const key = (v: unknown): v is number => Number.isInteger(v) && (v as number) >= 0 && (v as number) <= 127;
	const range = Array.isArray(set.range) && key(set.range[0]) && key(set.range[1]) && set.range[0] < set.range[1] ? set.range : null;
	return {
		range: range ?? [Math.min(48, ...pitches), Math.max(72, ...pitches)],
		scale: set.scale !== undefined && Object.hasOwn(SCALES, set.scale) ? set.scale : Object.keys(SCALES)[0],
		root: (NOTE_NAMES as readonly string[]).includes(set.root ?? '') ? set.root! : NOTE_NAMES[0],
		grid: set.grid !== undefined && Object.hasOwn(GRIDS, set.grid) ? set.grid : '1/4 beat'
	};
}

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

/** The slope a smooth curve has at the keyframe `i`, in value a beat, from the keyframes beside
 * it: at an end of the lane the slope of the line to its one neighbour, at a peak or in a valley
 * none, and else the mean of the two lines' slopes by their lengths, no more than twice the
 * lesser of them. `goofi_graph::timeline` is the owner; this mirrors it. */
function slope(keys: Key[], i: number): number {
	const line = (k: number): number => (keys[k + 1].value - keys[k].value) / (keys[k + 1].at - keys[k].at);
	if (i === 0 || i + 1 === keys.length) return line(Math.min(Math.max(i - 1, 0), keys.length - 2));
	const [before, after] = [line(i - 1), line(i)];
	if (before * after <= 0) return 0;
	const [left, right] = [keys[i].at - keys[i - 1].at, keys[i + 1].at - keys[i].at];
	const mean = (before * right + after * left) / (left + right);
	return Math.sign(before) * Math.min(Math.abs(mean), 2 * Math.abs(before), 2 * Math.abs(after));
}
/** The curve the value travels by from the keyframe `i` to the next one: the keyframe's own, and
 * for `smooth` the one cubic that leaves the keyframe at its slope and comes to the next at that
 * one's. The last keyframe has no next, and its curve is drawn as a straight line. */
export function shapeOf(keys: Key[], i: number): Curve {
	const [from, to] = [keys[i], keys[i + 1]];
	if (from.curve !== 'smooth') return from.curve;
	if (!to || to.value === from.value) return 'linear';
	const share = (k: number): number => (slope(keys, k) * (to.at - from.at)) / (to.value - from.value);
	return { custom: [[0, 0, 0, 0, 1 / 3, share(i) / 3], [1, 1, -1 / 3, -share(i + 1) / 3, 0, 0]] };
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
		const [next, shape] = [keys[i + 1], shapeOf(keys, i)];
		for (let s = 0; s < (next ? steps : 1); s++) {
			const f = s / steps;
			points.push(`L${at(along(k.at + ((next?.at ?? k.at) - k.at) * f), k.value + ((next?.value ?? k.value) - k.value) * curveAt(shape, f))}`);
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

/** The most of a range bar that a view past one end of the axis folds into, and how far past
 * that end, in lengths of the axis, the view is when the fold is half of that. */
const [FOLD_MOST, FOLD_HALF] = [0.4, 5];
/** Where a point of an axis from 0 to `span` stands on a bar of the axis's length, 0 the bar's
 * start and 1 its end. Past an end the place goes on in proportion to how far past the point is,
 * a twelfth of the bar for a length of the axis, and that for about the first length; then less
 * and less, and never as far as `FOLD_MOST`: a view with no widest folds into the bar's end. */
export function barPlace(at: number, span: number): number {
	const fold = (past: number): number => (FOLD_MOST * past) / (past + FOLD_HALF);
	return at < 0 ? -fold(-at / span) : at > span ? 1 + fold(at / span - 1) : at / span;
}
/** The point of the axis that stands at a place on the bar: `barPlace` the other way round. A
 * place past where a fold ends is the furthest point a fold holds. */
export function barPoint(on: number, span: number): number {
	const unfold = (by: number): number => (FOLD_HALF * Math.min(by, FOLD_MOST * 0.999)) / (FOLD_MOST - Math.min(by, FOLD_MOST * 0.999));
	return span * (on < 0 ? -unfold(-on) : on > 1 ? 1 + unfold(on - 1) : on);
}

/** The pixels a wheel's line is taken for, where a browser counts a turn in lines. */
const LINE = 16;
/** How far a turn of the wheel goes along an axis `length` pixels long, in pixels; 0 where the
 * turn is not along it. The axis runs to the right, or down where `down` says so. A wheel with
 * one direction turns along a sideways axis with Shift held: a browser that does not say so in
 * the turn itself says it with the key. A turn that goes across the axis as far or further is
 * not along it, so a slanted swipe scrolls one way. */
export function turned(e: { deltaX: number; deltaY: number; deltaMode: number; shiftKey: boolean }, down: boolean, length: number): number {
	const [x, y] = e.shiftKey && e.deltaX === 0 ? [e.deltaY, 0] : [e.deltaX, e.deltaY];
	const [along, across] = down ? [y, x] : [x, y];
	if (Math.abs(along) <= Math.abs(across)) return 0;
	return along * wheelUnit(e.deltaMode, length);
}
/** The pixels one count of a wheel's turn is: a pixel, a line, or a page as long as the axis. */
export const wheelUnit = (deltaMode: number, length: number): number => (deltaMode === 1 ? LINE : deltaMode === 2 ? length : 1);

/** The narrowest a view gets and the widest, in beats. */
export const [NARROWEST_VIEW, WIDEST_VIEW] = [0.05, 1e6];
/** How much of a view a pixel of a wheel's turn takes or gives, as a power of e. */
const ZOOM = 0.003;
/** A view after a wheel turned `pixels` to zoom about `about`, a point of the axis that stays
 * where it is in the panel: a turn up or away, which counts below 0, looks closer. */
export function zoomed([from, to]: [number, number], about: number, pixels: number): [number, number] {
	const width = Math.min(WIDEST_VIEW, Math.max(NARROWEST_VIEW, (to - from) * Math.exp(pixels * ZOOM)));
	const before = (about - from) / (to - from);
	return [about - before * width, about + (1 - before) * width];
}

/** What a recording has taken so far, flat `at, value` pairs, as a path in the box `lanePath`
 * draws in: a line from each sample to the next, begun again where the time goes back, as
 * another pass of the recording does. */
export function takePath(flat: number[], [lo, hi]: [number, number], along: (at: number) => number, flip = false): string {
	const points: string[] = [];
	for (let i = 0; i + 1 < flat.length; i += 2) {
		const [a, c] = [Number((1000 * along(flat[i])).toFixed(1)), Number((100 * ((flat[i + 1] - lo) / (hi - lo || 1))).toFixed(1))];
		points.push(`${i === 0 || flat[i] < flat[i - 2] ? 'M' : 'L'}${flip ? `${c} ${a}` : `${a} ${100 - c}`}`);
	}
	return points.join(' ');
}
