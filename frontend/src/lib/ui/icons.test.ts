import { describe, it, expect } from 'vitest';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { ICONS } from './icons';
import { CHROME_ICONS } from 'panelty';

/* `icons.ts` is vendored Lucide path geometry. This app's table is judged here, and every name the
 * source renders must resolve in the merge with `panelty`'s table. */

const SRC = fileURLToPath(new URL('../..', import.meta.url));

/** The SVG elements Lucide's geometry is drawn from. Anything else is not a vendored icon. */
const DRAW = /^(path|circle|rect|line|polyline|polygon|ellipse)$/;

/** The merged table, as the renderer resolves it — only the resolution question reads this. */
const ALL: Record<string, string> = { ...CHROME_ICONS, ...ICONS };
/** This app's own geometry: what the shape rules below are about. */
const names = Object.keys(ICONS);

/** Every `.ts`/`.svelte` under `src/`, except this guard and the table it reads. */
function sourceFiles(dir: string): string[] {
	const out: string[] = [];
	for (const entry of readdirSync(dir)) {
		const p = join(dir, entry);
		if (statSync(p).isDirectory()) out.push(...sourceFiles(p));
		else if (/\.(ts|svelte)$/.test(entry) && !/^icons\.(ts|test\.ts)$/.test(entry)) out.push(p);
	}
	return out;
}

const allSource = (): string =>
	sourceFiles(SRC)
		.map((p) => readFileSync(p, 'utf8'))
		.join('\n');

/** Every icon name the source renders, read from the three shapes that name one: the `<Icon name>`
 *  attribute as text or as an expression, and an `icon:` record field. */
function rendered(): Set<string> {
	const src = allSource();
	const used = new Set<string>();
	for (const [, tag] of src.matchAll(/<Icon\b([^>]*)>/g)) {
		for (const [, lit] of tag.matchAll(/\bname="([\w-]+)"/g)) used.add(lit);
		for (const [, expr] of tag.matchAll(/\bname=\{([^}]*)\}/g))
			for (const [, lit] of expr.matchAll(/'([\w-]+)'/g)) used.add(lit);
	}
	// A record field is bounded by its comma; every quoted literal in it is an icon name.
	for (const [, inRecord] of src.matchAll(/\bicon:\s*([^,\n]+)/g))
		for (const [, lit] of inRecord.matchAll(/'([\w-]+)'/g)) used.add(lit);
	return used;
}

describe('vendored Lucide geometry', () => {
	it('is drawing elements only, and at least one for every name', () => {
		expect(names.length, 'the table is not empty').toBeGreaterThan(0);
		const offenders: string[] = [];
		for (const n of names) {
			// Self-closing drawing tags back to back and nothing else: what makes the `{@html}` in
			// `Icon.svelte` safe.
			const geometry: string = ICONS[n as keyof typeof ICONS];
			const rest = geometry.replace(/<(\w+)((?:\s+[\w-]+="[^"]*")*)\s*\/>/g, (_, tag: string) =>
				DRAW.test(tag) ? '' : `<${tag}>`
			);
			if (rest !== '' || geometry === '') offenders.push(`${n}: ${rest}`);
		}
		expect(offenders).toEqual([]);
	});

	it('bakes in no paint of its own: colour comes from the control', () => {
		const offenders = names.filter((n) => /\b(fill|stroke|style|class|color)=/.test(ICONS[n as keyof typeof ICONS]));
		expect(offenders, 'no icon carries its own fill/stroke/style').toEqual([]);
	});

	it('vendors nothing the app does not render', () => {
		expect(names.filter((n) => !rendered().has(n))).toEqual([]);
	});

	// `Icon` takes a plain string, so a name with no geometry draws nothing and no type catches it.
	it('renders nothing it has no geometry for', () => {
		expect([...rendered()].filter((n) => !(n in ALL))).toEqual([]);
	});
});
