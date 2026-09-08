/** What seeds a param's expression: its own value as a Python literal, or a reference read as one. */
import type { ParamDescriptor } from '$lib/api/types';
import { splitReference } from './expr/refs';

export function literalFor(d: ParamDescriptor): string {
	if (d.type === 'pulse') return 'False';
	const v = d.value;
	if (typeof v === 'number') return String(v);
	if (typeof v === 'boolean') return v ? 'True' : 'False';
	return JSON.stringify(v);
}

/** The expression that reads the reference `node.slot` — one dropped node's link, said in Python.
 *  A node with ONE output is read bare, the spelling its own completion offers. */
export function expressionFor(reference: string, outputs: number): string {
	const [node, slot] = splitReference(reference);
	return outputs === 1 ? `nd('${node}')` : `nd('${node}').out.${slot}`;
}
