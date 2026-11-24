import { DEFAULT_KIND } from '$lib/api/vocab';

/** Map a goofi DataType (ARRAY/STRING/TABLE/…) to a CSS color variable. */
export function dtypeColor(dtype: string | undefined | null): string {
	const d = (dtype ?? '').toUpperCase();
	return Object.hasOwn(DEFAULT_KIND, d) ? `var(--dtype-${d.toLowerCase()})` : 'var(--text-muted)';
}

export function formatName(s: string): string {
	return s.replace(/_/g, ' ').replace(/\b\w/g, (m) => m.toUpperCase());
}
