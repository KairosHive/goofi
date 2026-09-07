/** The add-node menu's facets, and the ranking of its search on the bare name. */
import type { NodeTypeInfo } from '$lib/api/control';
import { bareName, engineOf } from './typeId';
import { nodeTypeSource } from './nodeTypeSource';

/** The palette tab that facets nothing. */
export const ALL_TAB = 'all';
/** The tab a plugin wears instead of its engine's, so the two never list one type twice. */
export const VST_TAB = 'vst';
/** …and the one the user's OWN nodes wear: the private library's and this patch's alike, so
 *  saving a node to the library does not move it out from under the reader. */
export const CUSTOM_TAB = 'custom';

/** Match quality, best first. */
const TIER = {
	none: 0,
	doc: 1,
	source: 2,
	tags: 3,
	nameSubstring: 4,
	nameWord: 5, // query starts a CamelCase / separated word inside the name
	namePrefix: 6,
	nameExact: 7
} as const;

const isUpper = (c: string): boolean => c >= 'A' && c <= 'Z';
const isLower = (c: string): boolean => c >= 'a' && c <= 'z';
const isLetter = (c: string): boolean => isUpper(c) || isLower(c);
const isDigit = (c: string): boolean => c >= '0' && c <= '9';

/** Is index `i` the first character of a word in `name`? */
function isWordStart(name: string, i: number): boolean {
	if (i === 0) return true;
	const c = name[i];
	const prev = name[i - 1];
	const next = name[i + 1];
	if (isLetter(c) && !isLetter(prev)) return true; // after separator / digit
	if (isUpper(c) && isLower(prev)) return true; // camelCase hump: ...oU...
	if (isUpper(c) && isUpper(prev) && next !== undefined && isLower(next)) return true; // acronym tail: OSC|Out
	if (isDigit(c) && !isDigit(prev)) return true; // letters → digit
	return false;
}

function hasWordStartMatch(name: string, lowerName: string, q: string): boolean {
	for (let i = 0; i <= name.length - q.length; i++) {
		if (isWordStart(name, i) && lowerName.startsWith(q, i)) return true;
	}
	return false;
}

function tierFor(t: NodeTypeInfo, q: string): number {
	const bare = bareName(t.type);
	const name = bare.toLowerCase();
	if (name === q) return TIER.nameExact;
	if (name.startsWith(q)) return TIER.namePrefix;
	if (hasWordStartMatch(bare, name, q)) return TIER.nameWord;
	if (name.includes(q)) return TIER.nameSubstring;
	if (t.tags.some((tag) => tag.includes(q))) return TIER.tags;
	if (t.bundle?.includes(q) || nodeTypeSource(t).includes(q)) return TIER.source;
	if (t.doc.toLowerCase().includes(q)) return TIER.doc;
	return TIER.none;
}

/** Rank `types` for `rawQuery`, best match first. An empty query returns the input order. */
export function rankNodeTypes(types: NodeTypeInfo[], rawQuery: string): NodeTypeInfo[] {
	const q = rawQuery.trim().toLowerCase();
	if (!q) return types;

	const scored = types
		.map((t) => {
			const name = bareName(t.type);
			const idx = name.toLowerCase().indexOf(q);
			return { t, name, tier: tierFor(t, q), idx: idx < 0 ? Number.MAX_SAFE_INTEGER : idx };
		})
		.filter((s) => s.tier !== TIER.none);

	scored.sort((a, b) => {
		if (a.tier !== b.tier) return b.tier - a.tier; // higher tier first
		if (a.idx !== b.idx) return a.idx - b.idx; // earlier in-name match first
		if (a.name.length !== b.name.length) return a.name.length - b.name.length; // tighter
		return a.name.localeCompare(b.name); // stable, alphabetical
	});

	return scored.map((s) => s.t);
}

/** The tab `t` belongs to: `vst` where an engine found it on its own account, `custom` where the
 *  user wrote it, else its engine. */
export function tabOf(t: NodeTypeInfo): string | null {
	if (t.source === 'plugin') return VST_TAB;
	if (t.source === 'custom' || t.source === 'patch') return CUSTOM_TAB;
	return engineOf(t.type);
}

/** The tabs `types` offer, `all` first. A structural type has no engine and so no tab of its own. */
export function paletteTabs(types: NodeTypeInfo[]): string[] {
	const tabs: string[] = [];
	for (const t of types) {
		const tab = tabOf(t);
		if (tab && !tabs.includes(tab)) tabs.push(tab);
	}
	// Neither a plugin format nor the user's own folder is an engine, so both sit after every
	// engine's tab. `sort` is stable, so the engines keep the order they were met in.
	const rank = (t: string): number => (t === VST_TAB ? 2 : t === CUSTOM_TAB ? 1 : 0);
	return [ALL_TAB, ...tabs.sort((a, b) => rank(a) - rank(b))];
}

/** The tab a fresh menu opens on: the engine of the node a slot click seeded it from, else the tab
 *  the user was last on. */
export function openingTab(seedType: string | null, lastTab: string): string {
	return (seedType && engineOf(seedType)) || lastTab;
}

/** `types` on one tab. `all` keeps every type, a structural one included. */
export function byTab(types: NodeTypeInfo[], tab: string): NodeTypeInfo[] {
	if (tab === ALL_TAB) return types;
	return types.filter((t) => tabOf(t) === tab);
}
