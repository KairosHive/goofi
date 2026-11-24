/** Per-variant virtual-keyboard and editing hints. `search` is the IDENTIFIER variant — a node,
 * tab or variable name is machine-read — and `path` maps to the `url` keyboard, which carries `/`. */

export type InputModeVariant = 'text' | 'search' | 'path';

const MACHINE = { autocapitalize: 'off', autocorrect: 'off', spellcheck: 'false' };

/** A plain attribute bag: `autocorrect` is Safari's non-standard attribute and has no typed slot. */
export const MODE_ATTRS: Record<InputModeVariant, Record<string, string>> = {
	text: {
		inputmode: 'text',
		enterkeyhint: 'done',
		autocapitalize: 'sentences',
		autocorrect: 'on',
		spellcheck: 'true'
	},
	search: { inputmode: 'search', enterkeyhint: 'search', ...MACHINE },
	path: { inputmode: 'url', enterkeyhint: 'go', ...MACHINE }
};
