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

/** Compact single-line form, used for lists and anything nested inside one. */
export function formatMetaInline(v: unknown): string {
	if (isList(v)) {
		const arr = Array.from(v as ArrayLike<unknown>);
		return '[' + arr.map(formatMetaInline).join(', ') + ']';
	}
	if (isPlainObject(v)) {
		return '{' + Object.entries(v).map(([k, x]) => `${k}: ${formatMetaInline(x)}`).join(', ') + '}';
	}
	return String(v);
}

/** Undo a viewer reduction's `meta.reduced` artifacts, so the inspector shows what the node
 * produced: the axis labels as they were before the reduction. The shape is the frame's own,
 * never the meta's; `originShape` in the viewers undoes the reduction there. Meta with no
 * `reduced` block comes back as-is. */
export function reconstructMeta(meta: Record<string, unknown>): Record<string, unknown> {
	const reduced = meta['reduced'];
	if (!isPlainObject(reduced)) return meta;
	const out: Record<string, unknown> = { ...meta };
	delete out['reduced'];
	const channels = isPlainObject(meta['channels'])
		? { ...(meta['channels'] as Record<string, unknown>) }
		: null;
	if (channels) {
		for (const [axisStr, info] of Object.entries(reduced)) {
			if (!isPlainObject(info)) continue;
			const key = 'dim' + axisStr;
			if (Array.isArray(info['orig_coord'])) channels[key] = info['orig_coord'];
			else delete channels[key];
		}
		out['channels'] = channels;
	}
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
	return String(value);
}
