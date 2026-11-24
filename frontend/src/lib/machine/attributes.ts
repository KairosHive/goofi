/** An attribute as a node's param: the faces the inspector offers, and the descriptor that draws a
 * value with the param widget of the attribute's kind. */
import type { Attribute, AttributeKind, Literal } from '$lib/api/generated';
import type { ParamDescriptor } from '$lib/api/types';
import { KIND, bornValue } from '$lib/panels/controlLayout';

export type Face = 'number' | 'vector' | 'color' | 'envelope' | 'toggle' | 'text' | 'select';

const num = (face: 'plain' | 'color' | 'envelope', vmax = 1): AttributeKind => ({ type: 'num', vmin: 0, vmax, int: false, face });

/** Every face, with the kind and the default an attribute is born with when it takes it: the
 * defaults are the control panel's, so a widget placed anywhere starts the same. */
export const FACES: { id: Face; kind: AttributeKind; born: Literal }[] = [
	{ id: 'number', kind: num('plain'), born: bornValue(KIND.knob) },
	{ id: 'vector', kind: num('plain'), born: bornValue(KIND.vector) },
	{ id: 'color', kind: num('color'), born: bornValue(KIND.color) },
	{ id: 'envelope', kind: num('envelope', 10), born: bornValue(KIND.envelope) },
	{ id: 'toggle', kind: { type: 'bool' }, born: bornValue(KIND.toggle) },
	{ id: 'text', kind: { type: 'string' }, born: bornValue(KIND.text) },
	{ id: 'select', kind: { type: 'string', options: ['a', 'b'] }, born: 'a' }
];

/** The face an attribute wears, from its kind and the shape of its default. */
export function faceOf(a: Attribute): Face {
	const k = a.kind;
	if (k.type === 'bool') return 'toggle';
	if (k.type === 'string') return k.options?.length ? 'select' : 'text';
	if (k.face !== 'plain') return k.face;
	return Array.isArray(a.default) && a.default.length > 1 ? 'vector' : 'number';
}

/** The part of a constant param's descriptor that is the same for every literal. */
const constant = (fallback: Literal | null) => ({
	doc: null,
	default: fallback,
	section: 0,
	show: [],
	role: null,
	refreshable: false,
	mode: 'constant' as const,
	expression: null,
	triggers: false,
	error: null
});

/** A bare number as a param with a slider over `vmin..vmax`: a transition's or a state's setting,
 * driven by `expression` where one is given. */
export function numberDescriptor(value: number, vmin: number, vmax: number, fallback = value, expression = ''): ParamDescriptor {
	const source = expression ? { mode: 'expression' as const, expression } : {};
	return { ...constant(fallback), ...source, type: 'num', value, vmin, vmax, int: false, options: [], face: 'plain' };
}

/** Four ADSR numbers as a constant envelope param, its times up to `vmax` seconds and its
 * release down to -1, the hold: a state's
 * envelope over one of its attributes. */
export function envelopeDescriptor(value: number[], vmax: number): ParamDescriptor {
	return { ...constant(null), type: 'num', value, vmin: -1, vmax, int: false, options: [], face: 'envelope' };
}

/** `value` of attribute `a` as a constant param, for ParamField to draw. */
export function descriptorFor(a: Attribute, value: Literal): ParamDescriptor {
	const base = constant(a.default);
	const k = a.kind;
	switch (k.type) {
		case 'num':
			return { ...base, type: 'num', value: value as number | number[], vmin: k.vmin, vmax: k.vmax, int: k.int, options: [], face: k.face };
		case 'bool':
			return { ...base, type: 'bool', value: Boolean(value) };
		default:
			return { ...base, type: 'string', value: String(value), options: k.options ?? null };
	}
}

/** What a param widget's commit becomes in the document: a bool is the number it reads as. */
export function literalOf(v: unknown): Literal {
	return typeof v === 'boolean' ? (v ? 1 : 0) : (v as Literal);
}

/** Where `value` stands in the cell's height, 0 the bottom and 1 the top: a number across its
 * range, a bool on or off, an option by its place in the list from the top; null where the face
 * is not a slider, such as a colour or a text. With `dim`, that entry of a vector. */
export function fractionOf(a: Attribute, value: Literal, dim = 0): number | null {
	const k = a.kind;
	if (k.type === 'bool') return Number(value) > 0 ? 1 : 0;
	if (k.type === 'string') {
		const n = k.options?.length ?? 0;
		if (n < 2) return null;
		const at = k.options!.indexOf(String(value));
		return at < 0 ? null : 1 - at / (n - 1);
	}
	if (k.face !== 'plain') return null;
	const v = Array.isArray(value) ? Number(value[dim] ?? 0) : Number(value);
	const span = k.vmax - k.vmin || 1;
	return Math.min(1, Math.max(0, (v - k.vmin) / span));
}

/** The value a pointer at `fraction` of the cell's height paints: the inverse of `fractionOf`,
 * an integer rounded, a vector's entry `dim` replaced in `held`; null where nothing is painted. */
export function valueAt(a: Attribute, fraction: number, held: Literal, dim = 0): Literal | null {
	const f = Math.min(1, Math.max(0, fraction));
	const k = a.kind;
	if (k.type === 'bool') return f > 0.5 ? 1 : 0;
	if (k.type === 'string') {
		const n = k.options?.length ?? 0;
		if (n < 2) return null;
		return k.options![Math.round((1 - f) * (n - 1))];
	}
	if (k.face !== 'plain') return null;
	const raw = k.vmin + f * (k.vmax - k.vmin);
	const v = k.int ? Math.round(raw) : Number(raw.toFixed(3));
	if (Array.isArray(held)) return held.map((x, i) => (i === dim ? v : x));
	return v;
}

/** A value in a cell's few characters: a number short, a list as its numbers, a text as it is. */
export function brief(value: Literal): string {
	if (typeof value === 'number') return Number.isInteger(value) ? String(value) : value.toFixed(2);
	if (typeof value === 'boolean') return value ? 'on' : 'off';
	if (Array.isArray(value)) return value.map((v) => brief(v as Literal)).join(' ');
	return String(value);
}
