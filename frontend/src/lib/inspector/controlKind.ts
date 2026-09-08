/** Descriptor → the inspector control that renders it. The param's TYPE alone decides: the row
 *  wears the same control in every mode, disabled where a source drives it. */
import type { ParamDescriptor } from '$lib/api/types';

export type ControlKind = 'pulse' | 'numeric' | 'toggle' | 'select' | 'text' | 'unknown';

export function controlKind(descriptor: ParamDescriptor): ControlKind {
	switch (descriptor.type) {
		case 'pulse':
			return 'pulse';
		case 'float':
		case 'int':
			return 'numeric';
		case 'bool':
			return 'toggle';
		case 'string':
			return (descriptor.options?.length ?? 0) > 0 || descriptor.refreshable ? 'select' : 'text';
		default:
			return 'unknown';
	}
}
