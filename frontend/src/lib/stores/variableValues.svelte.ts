/* The live value of every variable someone watches, read through the data worker as a slot's frame
 * is: `/data/variables/<name>`. The document carries no value, so this is the one place a panel,
 * a widget or a chip reads one from. */
import { bindViewer } from '$lib/api/frames';
import type { Literal } from '$lib/api/generated';
import { isArrayFrame, isStringFrame, type ArrayData, type DataFrame } from '$lib/codec/decode';
import { SvelteMap } from 'svelte/reactivity';

/** The latest frame per watched variable; absent until its first frame lands. Keyed, so a frame
 * wakes the readers of its own variable alone, and held as it came, never proxied. */
const frames = new SvelteMap<string, DataFrame>();
/** Each frame's literal, built once however often a template reads it. */
const literals = new WeakMap<DataFrame, Literal | null>();
const readers = new Map<string, { count: number; off: () => void }>();

function hold(name: string): void {
	const r = readers.get(name);
	if (r) {
		r.count += 1;
		return;
	}
	const off = bindViewer('variables', name, `variable:${name}`, null, (f) => {
		frames.set(name, f);
	});
	readers.set(name, { count: 1, off });
}

function release(name: string): void {
	const r = readers.get(name);
	if (!r) return;
	r.count -= 1;
	if (r.count > 0) return;
	r.off();
	readers.delete(name);
	frames.delete(name);
}

/** Keep every variable `names()` lists subscribed for as long as the calling component lives. A
 * list that moves holds what it gained before it lets go of what it lost, so a kept name never
 * drops its stream or its value. */
export function watchVariables(names: () => string[]): void {
	let held = new Set<string>();
	$effect(() => {
		const next = new Set(names());
		for (const n of next) if (!held.has(n)) hold(n);
		for (const n of held) if (!next.has(n)) release(n);
		held = next;
	});
	$effect(() => () => {
		for (const n of held) release(n);
		held = new Set();
	});
}

/** The latest frame of a watched variable, or null before it lands. */
export function variableFrame(name: string): DataFrame | null {
	return frames.get(name) ?? null;
}

/** The latest value of a watched variable as a literal, or null before it lands. */
export function variableValue(name: string): Literal | null {
	const f = variableFrame(name);
	if (!f) return null;
	if (!literals.has(f)) literals.set(f, literalOf(f));
	return literals.get(f) ?? null;
}

/** The pad array a watched variable holds, or null when it holds anything else. */
export function variableImage(name: string): ArrayData | null {
	const f = variableFrame(name);
	return f && formOf(f) === 'image' ? (f.data as ArrayData) : null;
}

/** A frame as the literal it is: a number for one element, a nested list for a wider array, the text. */
function literalOf(frame: DataFrame | null): Literal | null {
	if (!frame) return null;
	if (isStringFrame(frame)) return frame.data;
	if (!isArrayFrame(frame)) return null;
	const { shape, values } = frame.data;
	if (shape.length === 1 && shape[0] === 1) return values[0];
	let at = 0;
	const nest = (dim: number): Literal[] =>
		Array.from({ length: shape[dim] }, () => (dim === shape.length - 1 ? values[at++] : nest(dim + 1)));
	return shape.length === 0 ? values[0] : nest(0);
}

/** The form a frame holds: a `number`, a `text`, an `image` (a pad's `[h, w, 4]`), or a `list`;
 * a number before the first frame lands. */
export type Form = 'number' | 'text' | 'list' | 'image';
export function formOf(frame: DataFrame | null): Form {
	if (!frame || !isArrayFrame(frame)) return frame ? 'text' : 'number';
	const { shape } = frame.data;
	if (shape.length <= 1 && (shape[0] ?? 1) === 1) return 'number';
	return shape.length === 3 && shape[2] === 4 ? 'image' : 'list';
}
export const variableForm = (name: string): Form => formOf(variableFrame(name));
