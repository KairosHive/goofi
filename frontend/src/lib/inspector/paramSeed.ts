/** What seeds a param's expression: its own value as a Python literal. */
import type { ParamDescriptor, ParamMode, SourcePatch } from '$lib/api/types';

export function literalFor(d: ParamDescriptor): string {
	if (d.type === 'pulse') return 'False';
	const v = d.value;
	if (typeof v === 'number') return String(v);
	if (Array.isArray(v)) return `[${v.join(', ')}]`;
	if (typeof v === 'boolean') return v ? 'True' : 'False';
	return JSON.stringify(v);
}

/** The source switch's face per mode, shared by the row's switch and the form's filter strip. */
export const MODE_FACE: Record<ParamMode, { label: string; name: string }> = {
	constant: { label: 'C', name: 'Constant' },
	expression: { label: 'E', name: 'Expression' }
};

/** Restore a saved source, or seed a new expression from the value. */
export function sourceForMode(d: ParamDescriptor, mode: ParamMode): SourcePatch {
	return mode === 'expression' && !d.expression ? { expression: literalFor(d) } : { mode };
}
