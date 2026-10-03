/** Descriptor → the inspector control that renders it. The param's TYPE alone decides: the row
 *  wears the same control in every mode, disabled where a source drives it. */
import type { NumParam, ParamDescriptor } from '$lib/api/types';

export type ControlKind = 'pulse' | 'numeric' | 'vector' | 'color' | 'toggle' | 'select' | 'text' | 'unknown';

export const isNumeric = (d: ParamDescriptor): d is NumParam => d.type === 'num';

export function controlKind(descriptor: ParamDescriptor): ControlKind {
	if (isNumeric(descriptor)) {
		if (descriptor.color) return 'color';
		return Array.isArray(descriptor.value) && descriptor.value.length > 1 ? 'vector' : 'numeric';
	}
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
