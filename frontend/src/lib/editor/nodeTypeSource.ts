/** The one word a palette row carries beside a node's name: unavailability, else the bundle it was
 *  scanned from — the open patch and a plugin come from no root, so they name themselves. */
import type { NodeTypeInfo } from '$lib/api/control';

export function nodeTypeSource(t: NodeTypeInfo): string {
	return t.available ? (t.bundle ?? (t.source === 'patch' ? 'this patch' : t.source)) : 'unavailable';
}
