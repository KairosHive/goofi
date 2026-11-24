/** The clipboard payload: a graph fragment in the shape a `.gfi` carries, and its version. Putting
 * it on the platform clipboard is `$lib/clipboard`'s job; this module is only the shape. */
const CLIP_VERSION = 3; // bumped on a shape change, so an older tab's text is refused

export interface Clipboard {
	__goofi_clip__: number;
	doc: GraphFragment;
}

export interface GraphFragment {
	nodes: Record<string, { pos?: [number, number]; scope?: string }>;
	links?: Record<string, unknown>;
}

export function serializeClipboard(doc: GraphFragment): Clipboard {
	return { __goofi_clip__: CLIP_VERSION, doc };
}

/** Parse clipboard text; null if it isn't a goofi clipboard payload of this version. */
export function parseClipboard(text: string): Clipboard | null {
	let payload: unknown;
	try {
		payload = JSON.parse(text);
	} catch {
		return null;
	}
	const clip = payload as Clipboard | null;
	if (typeof clip !== 'object' || clip === null || clip.__goofi_clip__ !== CLIP_VERSION) return null;
	if (typeof clip.doc !== 'object' || clip.doc === null || typeof clip.doc.nodes !== 'object') return null;
	return clip;
}

/** The mean of `points`; the origin when there are none. */
export function centroid(points: [number, number][]): [number, number] {
	if (points.length === 0) return [0, 0];
	return [
		points.reduce((a, p) => a + (p[0] ?? 0), 0) / points.length,
		points.reduce((a, p) => a + (p[1] ?? 0), 0) / points.length
	];
}

/** The centre of a fragment's ROOTS, where a paste anchors. A record in a scope inside the fragment
 * is drawn in that scope's own space, not on the canvas the anchor is measured on. */
export function fragmentCentre(doc: GraphFragment): [number, number] {
	return centroid(Object.values(doc.nodes ?? {}).filter((n) => n.scope === undefined).map((n) => n.pos ?? [0, 0]));
}
