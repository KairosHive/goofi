/** The doc pane under the completion list: what the selected option is, and, when the cursor sits
 *  in a builtin's call, that call with the argument at the cursor marked. Plain DOM, because
 *  CodeMirror mounts the pane itself; the classes are themed in `theme.ts`. */
import type { ExprArgInfo, ExprBuiltinInfo } from '$lib/api/vocab';
import type { CallSite } from './complete';

/** What one option documents: a builtin whole, or a title and a line, maybe about one argument. */
export type Doc = { builtin: ExprBuiltinInfo } | { title: string; text: string; arg?: ExprArgInfo };

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

/** The pane for one option at one site. The site's call heads it, so an argument's or a value's
 *  option still shows the function it is for, with the argument under the cursor marked. */
export function renderDoc(doc: Doc, call?: CallSite): HTMLElement {
	const root = el('div', 'gx-doc');
	const own = 'builtin' in doc ? doc.builtin : null;
	if (call && call.fn !== own) {
		const head = el('div', 'gx-doc-site');
		head.appendChild(el('code', 'gx-doc-sig', call.fn.signature));
		head.appendChild(prose('gx-doc-text', call.fn.doc));
		if (call.fn.args.length) head.appendChild(argRows(call.fn, 'arg' in doc ? doc.arg : call.arg));
		root.appendChild(head);
	}
	const body = el('div', 'gx-doc-own');
	if ('builtin' in doc) {
		const own = doc.builtin;
		body.appendChild(el('code', 'gx-doc-sig', own.signature));
		body.appendChild(prose('gx-doc-text', own.doc));
		if (own.args.length) body.appendChild(argRows(own, call && call.fn === own ? call.arg : undefined));
	} else {
		body.appendChild(el('code', 'gx-doc-sig', doc.title));
		body.appendChild(prose('gx-doc-text', doc.text));
	}
	root.appendChild(body);
	return root;
}
