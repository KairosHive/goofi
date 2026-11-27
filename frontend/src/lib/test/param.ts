import type { NumParam } from '$lib/api/types';

/** A plain float param in 0..1 with every field the client reads, overridden where a test cares. */
export function numParam(over: Partial<NumParam> = {}): NumParam {
	return {
		type: 'num',
		value: 0,
		default: 0,
		vmin: 0,
		vmax: 1,
		int: false,
		options: [],
		face: 'plain',
		doc: null,
		refreshable: false,
		expression: null,
		mode: 'constant',
		error: null,
		section: 0,
		show: [],
		role: null,
		...over
	};
}
