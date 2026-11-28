import { describe, it, expect } from 'vitest';
import { EXPR_HIGHLIGHT_SPEC, EXPR_THEME_SPEC } from './theme';
import { WINDOW_STYLE } from './window';

/* The editor's styling is CSS-in-JS, outside the sweep of `theme/styleDrift.test.ts`. The colour
 * and font-size rules are asserted here over the spec objects and the window's sheet. */

/** Every `prop: value` leaf in a (possibly nested) StyleModule spec, with the selector path it sits
 *  under. `tag` is skipped: in a highlight spec it holds Lezer `Tag` objects — a graph, not a
 *  declaration — and walking into it does not terminate. */
function leaves(spec: object, path = ''): { path: string; prop: string; value: string }[] {
	const out: { path: string; prop: string; value: string }[] = [];
	for (const [key, value] of Object.entries(spec)) {
		if (key === 'tag') continue;
		if (value && typeof value === 'object') out.push(...leaves(value, path ? `${path} ${key}` : key));
		else out.push({ path, prop: key, value: String(value) });
	}
	return out;
}

/** Numeric literals in a value, `var()` references removed — styleDrift's `literals()`. */
function literals(value: string): string[] {
	let v = value;
	for (let prev = ''; v !== prev; ) {
		prev = v;
		v = v.replace(/var\(--[a-z0-9-]+\)/g, ' ');
	}
	return v.match(/-?\d*\.?\d+(?:px|rem|em|%|ch|vw|vh|pt)?/g) ?? [];
}

const THEME = leaves(EXPR_THEME_SPEC);
const COLOUR_LITERAL = /#[0-9a-f]{3,8}\b|\brgba?\(|\bhsla?\(/i;

describe('the editor theme speaks the token ladder', () => {
	it('names no colour literal: every ink has a name in app.css', () => {
		const offenders = [...THEME, ...leaves(EXPR_HIGHLIGHT_SPEC)]
			.filter((l) => COLOUR_LITERAL.test(l.value))
			.map((l) => `${l.path} { ${l.prop}: ${l.value} }`);
		expect(offenders).toEqual([]);
		expect(WINDOW_STYLE).not.toMatch(COLOUR_LITERAL);
		// The scan fires on a hex ink and on a functional one.
		const raw = leaves({ '.x': { color: '#ffffff', background: 'rgba(0,0,0,.5)' } });
		expect(raw.filter((l) => COLOUR_LITERAL.test(l.value))).toHaveLength(2);
	});

	it('routes every syntax colour through a --syn-* token', () => {
		for (const l of leaves(EXPR_HIGHLIGHT_SPEC).filter((l) => l.prop === 'color')) {
			expect(l.value, l.path).toMatch(/^var\(--(syn-[a-z]+|danger)\)$/);
		}
	});

	it('sizes type through --fs-*, never a raw literal', () => {
		// 16px is exempt: iOS force-zooms a focused control under it.
		const offenders = THEME.filter(
			(l) => l.prop === 'fontSize' && literals(l.value).some((n) => n !== '0' && n !== '16px')
		).map((l) => `${l.path} { ${l.prop}: ${l.value} }`);
		expect(offenders).toEqual([]);
		expect(literals('var(--fs-sm) 7px'), 'a token contributes nothing to read').toEqual(['7px']);
	});
});
