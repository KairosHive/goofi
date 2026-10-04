/* The live value of every variable someone watches, read through the data worker as a slot's frame
 * is: `/data/variables/<name>`. The document carries no value, so this is the one place a panel,
 * a widget or a chip reads one from. */
import { bindViewer } from '$lib/api/frames';
import type { Literal } from '$lib/api/generated';
import { isArrayFrame, isStringFrame, type ArrayData, type DataFrame } from '$lib/codec/decode';

/** The latest frame per watched variable; absent until its first frame lands. */
const frames = $state<Record<string, DataFrame>>({});
const readers = new Map<string, { count: number; off: () => void }>();

function hold(name: string): void {
	const r = readers.get(name);
	if (r) {
		r.count += 1;
		return;
	}
	const off = bindViewer('variables', name, `variable:${name}`, null, (f) => {
		frames[name] = f;
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
	delete frames[name];
}

/** Keep every variable `names()` lists subscribed for as long as the calling component lives. */
export function watchVariables(names: () => string[]): void {
	$effect(() => {
		const held = [...new Set(names())];
		for (const n of held) hold(n);
		return () => {
			for (const n of held) release(n);
		};
	});
}

/** The latest frame of a watched variable, or null before it lands. */
export function variableFrame(name: string): DataFrame | null {
	return frames[name] ?? null;
}

/** The latest value of a watched variable as a literal, or null before it lands. */
export function variableValue(name: string): Literal | null {
	return literalOf(variableFrame(name));
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
