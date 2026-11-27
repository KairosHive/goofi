import type { GraphSnapshot } from '$lib/api/control';

/** The session frame a `hello` or a `graph_replaced` carries, overridden where a test cares. */
export function snapshot(over: Partial<GraphSnapshot> = {}): GraphSnapshot {
	return {
		record: { running: false, folder: null, elapsed: null, streams: [], error: null },
		runtime: {},
		save_path: null,
		unsaved_changes: false,
		instance_id: 'sess1',
		doc_version: 0,
		viewpoint: null,
		...over
	};
}
