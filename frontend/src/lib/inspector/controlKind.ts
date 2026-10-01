/** Descriptor → the inspector control that renders it. The param's TYPE alone decides: the row
 *  wears the same control in every mode, disabled where a source drives it. */
import type { FloatParam, IntParam, ParamDescriptor } from '$lib/api/types';

export type ControlKind = 'pulse' | 'numeric' | 'toggle' | 'select' | 'text' | 'unknown';

export const isNumeric = (d: ParamDescriptor): d is FloatParam | IntParam => d.type === 'float' || d.type === 'int';

export function controlKind(descriptor: ParamDescriptor): ControlKind {
	if (isNumeric(descriptor)) return 'numeric';
	switch (descriptor.type) {
		case 'pulse':
			return 'pulse';
		case 'bool':
			return 'toggle';
		case 'string':
			return (descriptor.options?.length ?? 0) > 0 || descriptor.refreshable ? 'select' : 'text';
		default:
			return 'unknown';
	}
}
