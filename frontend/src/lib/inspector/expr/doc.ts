/** The window's doc: the builtin the cursor sits in with the argument at the cursor marked, or the
 *  scope's own line. Plain DOM, mounted by `window.ts`; the classes are themed in `theme.ts`. */
import type { ExprArgInfo, ExprBuiltinInfo } from '$lib/api/vocab';

/** What one option documents: a builtin whole, or a title and a line, maybe about one argument. */
export type Doc = { builtin: ExprBuiltinInfo } | { title: string; text: string; arg?: ExprArgInfo };

/** An option's one line, for its row. */
export function brief(doc: Doc): string {
	return 'builtin' in doc ? doc.builtin.doc : doc.text;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls: string, text?: string): HTMLElementTagNameMap[K] {
	const node = document.createElement(tag);
	node.className = cls;
	if (text !== undefined) node.textContent = text;
	return node;
}

/** A line of prose with `code` spans for its backticked runs. */
function prose(cls: string, text: string): HTMLElement {
	const p = el('p', cls);
	const parts = text.split('`');
	parts.forEach((part, i) => {
		if (i % 2 === 1) p.appendChild(el('code', 'gx-doc-code', part));
		else p.appendChild(document.createTextNode(part));
	});
	return p;
}

/** One builtin's arguments as rows: name, what it takes, its default and options, its line. */
function argRows(fn: ExprBuiltinInfo, mark?: ExprArgInfo): HTMLElement {
	const list = el('dl', 'gx-doc-args');
	for (const a of fn.args) {
		const row = el('div', `gx-doc-arg${mark && mark.name === a.name ? ' gx-doc-arg-at' : ''}`);
		const name = el('dt', 'gx-doc-arg-name', a.keyword ? `${a.name}=` : a.name);
		const facts = el('span', 'gx-doc-arg-facts');
		facts.appendChild(el('span', 'gx-doc-arg-kind', a.kind));
		if (a.default) facts.appendChild(el('span', 'gx-doc-arg-default', `= ${a.default}`));
		else facts.appendChild(el('span', 'gx-doc-arg-required', 'required'));
		name.appendChild(facts);
		row.appendChild(name);
		const dd = el('dd', 'gx-doc-arg-doc');
		dd.appendChild(prose('gx-doc-arg-text', a.doc));
		if (a.options.length) {
			const opts = el('span', 'gx-doc-options');
			for (const o of a.options) opts.appendChild(el('code', 'gx-doc-option', o));
			dd.appendChild(opts);
		}
		row.appendChild(dd);
		list.appendChild(row);
	}
	return list;
}

/** A builtin at the cursor: its signature, its line and its arguments, `mark` the one the cursor is at. */
export function renderBuiltin(fn: ExprBuiltinInfo, mark?: ExprArgInfo): HTMLElement {
	const root = el('div', 'gx-doc');
	root.appendChild(el('code', 'gx-doc-sig', fn.signature));
	root.appendChild(prose('gx-doc-text', fn.doc));
	if (fn.args.length) root.appendChild(argRows(fn, mark));
	return root;
}

/** A cursor in no call: what an expression of this scope reads. */
export function renderScope(machine: boolean): HTMLElement {
	const root = el('div', 'gx-doc');
	root.appendChild(el('code', 'gx-doc-sig', machine ? 'a machine’s expression' : 'a param’s expression'));
	root.appendChild(
		prose(
			'gx-doc-text',
			machine
				? 'Python over `me`, this playhead; `sm(\'name\')`, a state or a playhead; `nd(\'name\')`; `vars.<group>.<element>`; `t`; `lfo()`, `nse()`, `qtz()`; `np` and math.'
				: 'Python over `nd(\'name\')`; `sm(\'playhead\')`; `me`, this node; `vars.<group>.<element>`; `t`; `lfo()`, `nse()`, `qtz()`; `np` and math.'
		)
	);
	return root;
}
