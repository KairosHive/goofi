/** Pure formatting for the metadata inspector. */

function isTypedArray(v: unknown): v is ArrayLike<number> {
	return ArrayBuffer.isView(v) && !(v instanceof DataView);
}

function isList(v: unknown): v is ArrayLike<unknown> {
	return Array.isArray(v) || isTypedArray(v);
}

function isPlainObject(v: unknown): v is Record<string, unknown> {
	return v !== null && typeof v === 'object' && !Array.isArray(v) && !isTypedArray(v);
}

function formatScalar(v: unknown): string {
	if (v === null) return 'null';
	if (v === undefined) return 'undefined';
	if (typeof v === 'bigint') return v.toString();
	if (typeof v === 'string') return v;
	return String(v);
}

/** Compact single-line form, used for lists and anything nested inside one. */
export function formatMetaInline(v: unknown): string {
	if (isList(v)) {
		const arr = Array.from(v as ArrayLike<unknown>);
		return '[' + arr.map(formatMetaInline).join(', ') + ']';
	}
	if (isPlainObject(v)) {
		return '{' + Object.entries(v).map(([k, x]) => `${k}: ${formatMetaInline(x)}`).join(', ') + '}';
	}
	return formatScalar(v);
}

/** Undo a viewer reduction's `meta.reduced` artifacts, so the inspector shows what the node
 * produced. Meta with no `reduced` block comes back as-is. */
export function reconstructMeta(meta: Record<string, unknown>): Record<string, unknown> {
	const reduced = meta['reduced'];
	if (!isPlainObject(reduced)) return meta;
	const out: Record<string, unknown> = { ...meta };
	delete out['reduced'];
	// The 8-bit hop is the viewer's, never the node's: inside the graph a frame is f32.
	if (isPlainObject(reduced['depth'])) out['dtype'] = 'float32';
	const shape = Array.isArray(meta['shape']) ? [...(meta['shape'] as number[])] : null;
	const channels = isPlainObject(meta['channels'])
		? { ...(meta['channels'] as Record<string, unknown>) }
		: null;
	for (const [axisStr, info] of Object.entries(reduced)) {
		if (!isPlainObject(info)) continue;
		const axis = Number(axisStr);
		const origLen = typeof info['orig_len'] === 'number' ? (info['orig_len'] as number) : null;
		if (shape && origLen !== null && Number.isInteger(axis) && axis >= 0 && axis < shape.length) {
			shape[axis] = origLen;
		}
		if (channels) {
			const key = 'dim' + axisStr;
			if (Array.isArray(info['orig_coord'])) channels[key] = info['orig_coord'];
			else delete channels[key];
		}
	}
	if (shape) out['shape'] = shape;
	if (channels) out['channels'] = channels;
	return out;
}

/** Top-level entries of a meta dict, in insertion order, minus the `__*__` internal keys. */
export function metaEntries(meta: unknown): [string, unknown][] {
	if (!isPlainObject(meta)) return [];
	return Object.entries(reconstructMeta(meta)).filter(
		([k]) => !(k.startsWith('__') && k.endsWith('__'))
	);
}

/** Readable text for one meta value: lists inline, dicts indented multi-line. */
export function formatMetaValue(value: unknown, indent = 0): string {
	if (isList(value)) return formatMetaInline(value);
	if (isPlainObject(value)) {
		const entries = Object.entries(value);
		if (entries.length === 0) return '{}';
		const pad = '  '.repeat(indent);
		return entries
			.map(([k, v]) =>
				isPlainObject(v) && Object.keys(v).length > 0
					? `${pad}${k}:\n${formatMetaValue(v, indent + 1)}`
					: `${pad}${k}: ${formatMetaValue(v, indent + 1)}`
			)
			.join('\n');
	}
	return formatScalar(value);
}
