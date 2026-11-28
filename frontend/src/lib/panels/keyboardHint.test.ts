import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

/* The empty-canvas hint is the only `<kbd>` in the UI. The key it names must be the key whose
 * keydown branch opens the add-node menu. It is read from the source because the hint renders
 * only on an empty scope, which the e2e suite's shared patch never is. */

const SRC = readFileSync(
	fileURLToPath(new URL('./NodeEditorPanel.svelte', import.meta.url)),
	'utf8'
);

/** Escape a key name for embedding in a RegExp (`+` is a quantifier). */
function esc(s: string): string {
	return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

describe('the empty-canvas keyboard hint', () => {
	it('names exactly one key', () => {
		expect([...SRC.matchAll(/<kbd>([^<]+)<\/kbd>/g)].map((m) => m[1])).toHaveLength(1);
	});

	it('names a key whose keydown branch opens the add-node menu', () => {
		const key = /<kbd>([^<]+)<\/kbd>/.exec(SRC)![1];
		// `[^{]*\)`: a branch may carry further conjuncts after the key test.
		const branch = new RegExp(`e\\.key === '${esc(key)}'[^{]*\\)\\s*\\{([^}]*)`).exec(SRC);
		expect(branch, `no keydown branch in this file matches \`${key}\``).not.toBeNull();
		// Comments are stripped from the capture, so prose that spells `openAddMenu` does not satisfy it.
		const body = branch![1].replace(/\/\/.*$/gm, '');
		expect(body, `the \`${key}\` branch is not the add-node menu`).toContain('openAddMenu');
	});
});
