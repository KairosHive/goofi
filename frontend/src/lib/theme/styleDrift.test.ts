import { describe, it, expect } from 'vitest';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

/* The style vocabulary, read from the CSS on disk: `app.css` and every component's `<style>`
 * blocks and inline `style=` attributes. Three rules: a font size is a `--fs-*` token, a font
 * family is one of the two face tokens, and a tint that `app.css` names is used by its name. */

const slash = (p: string) => p.replaceAll('\\', '/');
const LIB = slash(fileURLToPath(new URL('..', import.meta.url)));
const APP_CSS = fileURLToPath(new URL('../../app.css', import.meta.url));

/** Files whose raw px sizes are exempt, each with its reason. */
const ALLOW_FILE: Record<string, string> = {
	'editor/GoofiNode.svelte': 'frozen node-canvas geometry (mirrored in nodeMetrics.ts/snap.ts)',
	'editor/BoundaryNode.svelte': 'frozen node-canvas geometry (mirrored in nodeMetrics.ts/snap.ts)',
	'editor/PlacementPreview.svelte': 'frozen node-canvas geometry (mirrors GoofiNode exactly)',
	'viewers/SlotViewer.svelte': 'frozen node-canvas geometry (sits inside a --node-u slot row)',
	'editor/PerfHud.svelte': 'dev diagnostic overlay, sized to be unobtrusive, not to the scale'
};

/** Sizes that are right as literals; `file` omitted means the reason is the value's own. */
const ALLOW_VALUE: { file?: string; value: RegExp; why: string }[] = [
	{ value: /^16px$/, why: 'iOS force-zooms a focused control below 16px: a device threshold, not a scale rung' },
	{ file: 'viewers/StringViewer.svelte', value: /em$/, why: 'rendered markdown sizes relative to the viewer root, which is on --fs-*' },
	{ file: 'app.css', value: /^(11px|14px|0\.9vw|0\.3vh)$/, why: 'the responsive root clamp is the rem every rung is measured in' }
];

/** Values that are no size at all. */
const INVARIANT = new Set(['0', '0px', '100%']);

/** Everything a `font-family` may say. `--panelty-font-sans` maps onto `--font-sans` in `app.css`. */
const FONT_TOKEN = new Set([
	'var(--font-sans)',
	'var(--font-mono)',
	'var(--panelty-font-sans, var(--panelty-font-sans-default))',
	'inherit'
]);

function svelteFiles(dir: string): string[] {
	const out: string[] = [];
	for (const entry of readdirSync(dir)) {
		const p = join(dir, entry);
		if (statSync(p).isDirectory()) out.push(...svelteFiles(p));
		else if (entry.endsWith('.svelte')) out.push(slash(p));
	}
	return out.sort();
}

const stripComments = (css: string): string => css.replace(/\/\*[\s\S]*?\*\//g, ' ');

/** One attribute's value, from its opening `"`, `'` or `{` to the delimiter that closes it. */
function attrValue(src: string, at: number): string {
	const open = src[at];
	if (open !== '{') {
		const end = src.indexOf(open, at + 1);
		return src.slice(at, end === -1 ? src.length : end + 1);
	}
	let depth = 0;
	let quote = '';
	let i = at;
	for (; i < src.length; i++) {
		const c = src[i];
		if (quote) {
			if (c === quote) quote = '';
		} else if (c === '"' || c === "'" || c === '`') quote = c;
		else if (c === '{') depth++;
		else if (c === '}' && --depth === 0) break;
	}
	return src.slice(at, i + 1);
}


/** A component's CSS: its `<style>` blocks, and each inline `style=` attribute as one rule. An
 *  interpolation is put in parentheses, so `{depth * 12}px` keeps its number. */
function componentCss(src: string): string {
	const blocks = [...src.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)].map((m) => m[1]);
	const markup = src.replace(/<script[\s\S]*?<\/script>/g, ' ').replace(/<style[\s\S]*?<\/style>/g, ' ');
	for (const m of markup.matchAll(/\bstyle=(?=["'{])/g)) {
		const value = attrValue(markup, m.index + m[0].length);
		blocks.push(`[style] { ${value.slice(1, -1).replace(/[{}]/g, (c) => (c === '{' ? '(' : ')'))} }`);
	}
	return stripComments(blocks.join('\n'));
}

/** Everything the guard scans: the central module, then every component's CSS. */
function sources(): { rel: string; css: string }[] {
	return [
		{ rel: 'app.css', css: stripComments(readFileSync(APP_CSS, 'utf8')) },
		...svelteFiles(LIB).map((p) => ({ rel: p.slice(LIB.length), css: componentCss(readFileSync(p, 'utf8')) }))
	];
}

/** The numeric literals in a declaration value, token references removed innermost first. A
 *  fallback that is a value stays, so `var(--select-fs, 16px)` still answers 16px. */
function literals(value: string): string[] {
	let v = value;
	const inner = /var\(\s*(--[a-z0-9-]+)\s*(?:,((?:[^()]|\([^()]*\))*))?\)/;
	for (let prev = ''; v !== prev; ) {
		prev = v;
		v = v.replace(inner, (_, __, fallback: string | undefined) => fallback ?? ' ');
	}
	return v.match(/-?\d*\.?\d+(?:px|rem|em|%|ch|vw|vh|pt)?/g) ?? [];
}

/** One source's `font-size` declarations that carry a raw literal. */
function sizeDrift(rel: string, css: string): string[] {
	const allowed = ALLOW_VALUE.filter((a) => a.file === undefined || a.file === rel);
	return [...css.matchAll(/(?:^|[;{}])\s*font-size\s*:\s*([^;}]+)/g)]
		.filter((m) =>
			literals(m[1]).some(
				(n) => !INVARIANT.has(n) && !(rel in ALLOW_FILE && n.endsWith('px')) && !allowed.some((a) => a.value.test(n))
			)
		)
		.map((m) => `${rel}  font-size: ${m[1].trim()}`);
}

/** One source's font families that are no token. An `@font-face` block names a face and is skipped;
 *  a `--x-font-family` custom property is a hook and is not read. */
function familyDrift(rel: string, css: string): string[] {
	return [...css.replace(/@font-face\s*\{[^}]*\}/g, ' ').matchAll(/(?:^|[;{}])\s*font-family\s*:\s*([^;}]+)/g)]
		.map((m) => m[1].replace(/\s+/g, ' ').trim())
		.filter((v) => !FONT_TOKEN.has(v))
		.map((v) => `${rel}  font-family: ${v}`);
}

/** A colour recipe without its whitespace and case, so two spellings of one mix compare equal. */
const recipe = (mix: string): string => mix.replace(/\s+/g, '').toLowerCase();
const colorMixes = (css: string): string[] => css.match(/color-mix\((?:[^()]|\([^()]*\))*\)/g) ?? [];

/** The derived tints `app.css` gives a name, keyed by their recipe. */
function namedRecipes(appCss: string): Map<string, string> {
	const out = new Map<string, string>();
	for (const [, name, mix] of appCss.matchAll(/(--[\w-]+)\s*:\s*(color-mix\([^;]*\))\s*;/g))
		out.set(recipe(mix), name);
	return out;
}

describe('style vocabulary', () => {
	it('reads app.css and the components, so no rule passes on an empty sweep', () => {
		const scanned = sources();
		expect(scanned.length).toBeGreaterThan(40);
		expect(scanned.find((s) => s.rel === 'app.css')?.css).toContain('--fs-');
	});

	it('sizes type through --fs-*, never a raw literal', () => {
		expect(sources().flatMap((s) => sizeDrift(s.rel, s.css))).toEqual([]);
		// The scan fires on a literal in a block, in an attribute and in a fallback.
		const raw = ['<style>.n { font-size: 13px; }</style>', '<p style="font-size: {n * 2}px"></p>', '<style>.n { font-size: var(--x, 13px) }</style>'];
		for (const src of raw) expect(sizeDrift('x.svelte', componentCss(src)), src).toHaveLength(1);
		expect(sizeDrift('x.svelte', componentCss('<style>.n { font-size: var(--fs-sm); --hook-font-size: 13px; }</style>'))).toEqual([]);
	});

	it('speaks type through --font-sans / --font-mono, never a raw family', () => {
		expect(sources().flatMap((s) => familyDrift(s.rel, s.css))).toEqual([]);
		// The scan fires on a raw family, and reads app.css past its @font-face blocks.
		const RAW = "'Comic Sans MS', cursive";
		expect(familyDrift('x.svelte', componentCss(`<div style="font-family: ${RAW}"></div>`))).toHaveLength(1);
		expect(familyDrift('app.css', `@font-face { font-family: 'Inter'; } body { font-family: ${RAW}; }`)).toHaveLength(1);
		for (const ok of FONT_TOKEN) expect(familyDrift('x.svelte', `.a { font-family: ${ok}; }`), ok).toEqual([]);
	});

	it('never re-spells a tint that already has a name in app.css', () => {
		const named = namedRecipes(readFileSync(APP_CSS, 'utf8'));
		expect(named.size, 'app.css still defines the derived tints this looks for').toBeGreaterThan(4);
		const offenders = sources()
			.filter((s) => s.rel !== 'app.css')
			.flatMap((s) =>
				colorMixes(s.css)
					.filter((mix) => named.has(recipe(mix)))
					.map((mix) => `${s.rel}  ${mix} is var(${named.get(recipe(mix))})`)
			);
		expect(offenders).toEqual([]);
		// The match reads through whitespace and case; another strength is another tint.
		const one = namedRecipes('--accent-fill: color-mix(in srgb, var(--accent) 18%, transparent);');
		expect(one.get(recipe('color-mix(in  SRGB,var(--accent)  18%,transparent)'))).toBe('--accent-fill');
		expect(one.has(recipe('color-mix(in srgb, var(--accent) 17%, transparent)'))).toBe(false);
	});
});
