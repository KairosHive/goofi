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

/** The parts of one param row, declared in one place from the descriptor and the reader's view.
 *  The markup renders what the plan names and decides nothing of its own. */
export type RowPlan = {
	/** The control on the row's face. A colour opened as entries wears a vector's numbers. */
	face: ControlKind;
	/** The L/I switch after the face: a list of entries can be read either way. */
	viewSwitch: boolean;
	/** The face cannot be edited: a source drives the whole, or an entry while the whole is shown. */
	disabled: boolean;
	/** Open: the whole list typed as one. Only a constant whole has a list to type. */
	list: boolean;
	/** Open: one row per entry, each with a source of its own. */
	elements: boolean;
	/** Open: the editor of the expression that drives the whole. The entry view shows the entries only. */
	source: boolean;
	/** Open: the whole's C/E, trigger and MIDI learn. Entry rows stand in for it. */
	foot: boolean;
};

export type RowView = {
	/** The reader opened the list as individual entries. */
	individual: boolean;
};

export function rowPlan(d: ParamDescriptor, view: RowView): RowPlan {
	const kind = controlKind(d);
	const entries = kind === 'color' || kind === 'vector';
	const individual = entries && view.individual;
	const driven = d.mode !== 'constant';
	const anyEntryDriven = (d.elements ?? []).some((e) => e.mode !== 'constant');
	const source = !individual && d.mode === 'expression';
	return {
		face: kind === 'color' && individual ? 'vector' : kind,
		viewSwitch: entries,
		disabled: driven || (entries && !individual && anyEntryDriven),
		list: entries && !individual && !source,
		elements: individual,
		source,
		foot: !individual
	};
}
