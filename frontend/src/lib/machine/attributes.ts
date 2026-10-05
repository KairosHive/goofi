/** An attribute as a node's param: the faces the inspector offers, and the descriptor that draws a
 * value with the param widget of the attribute's kind. */
import type { Attribute, AttributeKind, Literal } from '$lib/api/generated';
import type { ParamDescriptor } from '$lib/api/types';

export type Face = 'number' | 'vector' | 'color' | 'toggle' | 'text' | 'select';

/** Every face, with the kind and the default an attribute is born with when it takes it. */
export const FACES: { id: Face; kind: AttributeKind; born: Literal }[] = [
	{ id: 'number', kind: { type: 'num', vmin: 0, vmax: 1, int: false, color: false }, born: 0 },
	{ id: 'vector', kind: { type: 'num', vmin: 0, vmax: 1, int: false, color: false }, born: [0, 0, 0] },
	{ id: 'color', kind: { type: 'num', vmin: 0, vmax: 1, int: false, color: true }, born: [0, 0, 0, 1] },
	{ id: 'toggle', kind: { type: 'bool' }, born: 0 },
	{ id: 'text', kind: { type: 'string' }, born: '' },
	{ id: 'select', kind: { type: 'string', options: ['a', 'b'] }, born: 'a' }
];

/** The face an attribute wears, from its kind and the shape of its default. */
export function faceOf(a: Attribute): Face {
	const k = a.kind;
	if (k.type === 'bool') return 'toggle';
	if (k.type === 'string') return k.options?.length ? 'select' : 'text';
	if (k.color) return 'color';
	return Array.isArray(a.default) && a.default.length > 1 ? 'vector' : 'number';
}

/** `value` of attribute `a` as a constant param, for ParamField to draw. */
export function descriptorFor(a: Attribute, value: Literal): ParamDescriptor {
	const base = {
		doc: null,
		default: a.default,
		section: 0,
		show: null,
		role: null,
		refreshable: false,
		mode: 'constant' as const,
		expression: null,
		triggers: false,
		error: null
	};
	const k = a.kind;
	switch (k.type) {
		case 'num':
			return { ...base, type: 'num', value: value as number | number[], vmin: k.vmin, vmax: k.vmax, int: k.int, options: [], color: k.color };
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
