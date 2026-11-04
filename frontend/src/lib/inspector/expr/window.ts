/** The completion window: an extension of the code field, under it at its width, open while the
 *  field has focus. Two rows: the doc of what the cursor sits in, as tall as it is, and under it
 *  the options CodeMirror's completion holds, taking the rest, selected and accepted through
 *  CodeMirror's own state. */
import { EditorView, ViewPlugin } from '@codemirror/view';
import type { Extension } from '@codemirror/state';
import { acceptCompletion, completionStatus, currentCompletions, selectedCompletionIndex, setSelectedCompletion, type Completion } from '@codemirror/autocomplete';
import { MARGIN, overlayViewport } from 'panelty';
import { docSite, type GoofiCompletion } from './complete';
import { brief, renderBuiltin, renderScope } from './doc';
import type { ExprCatalogue } from './catalogue';

/** What the keys decide about the window: Escape dismisses it until Ctrl+Space or a new focus. */
export interface WindowRule {
	dismissed: boolean;
}

/** The least the window keeps when the viewport's bottom is close: then, and only then, it overflows. */
const MIN_HEIGHT = 280;

/* The window's look, mounted once: it lives in `document.body` outside the editor, and must not
   wear the editor's theme classes, whose root rule pins `display: flex !important`. */
export const WINDOW_STYLE = `
.gx-win { position: fixed; box-sizing: border-box; overflow: hidden; z-index: var(--z-menu);
	background: var(--surface-1); border: 1px solid var(--border); border-top: none;
	border-radius: 0 0 var(--radius-sm) var(--radius-sm); box-shadow: var(--shadow-1); color: var(--text);
	font-family: var(--font-sans); font-size: var(--fs-small); line-height: var(--lh-text);
	/* The doc row is its content's height, up to what leaves the list three rows; the list row takes the rest. */
	display: grid; grid-template-rows: minmax(0, auto) minmax(calc(3 * (var(--lh-text) * 1em + 2 * var(--space-1)) + var(--space-4)), 1fr); }
.gx-win[hidden] { display: none; }
.gx-win-doc { min-height: 0; overflow-y: auto; padding: var(--space-4); background: var(--surface-2); border-bottom: 1px solid var(--border); }
/* The list is a table: a name column, a line column that gives way, a kind column; the rows subgrid
   onto it, so every column lines up down the list and a row still paints its selection. */
.gx-win-list { min-height: 0; overflow-y: auto; margin: 0; padding: var(--space-4) 0 0; list-style: none;
	display: grid; grid-template-columns: max-content minmax(0, 1fr) max-content; align-content: start; }
.gx-win-stale { opacity: 0.6; }
.gx-win-item { display: grid; grid-column: 1 / -1; grid-template-columns: subgrid; align-items: baseline;
	column-gap: var(--space-4); padding: var(--space-1) var(--space-4); cursor: default; white-space: nowrap; }
.gx-win-item[aria-selected] { background: var(--accent-fill); }
.gx-win-label { font-family: var(--font-mono); color: var(--syn-name); }
.gx-win-function { color: var(--syn-function); }
.gx-win-keyword { color: var(--syn-keyword); }
.gx-win-constant { color: var(--syn-literal); }
.gx-win-brief { min-width: 0; overflow: hidden; text-overflow: ellipsis; color: var(--text-muted); }
.gx-win-detail { max-width: 14rem; overflow: hidden; text-overflow: ellipsis; color: var(--text-dim); font-size: var(--fs-micro); text-align: right; }
.gx-doc { display: flex; flex-direction: column; }
.gx-doc-sig { display: block; font-family: var(--font-mono); color: var(--syn-function); font-weight: 600; }
.gx-doc-text { margin: var(--space-1) 0 0; color: var(--text); }
.gx-doc-code, .gx-doc-option { font-family: var(--font-mono); color: var(--syn-string); }
.gx-doc-args { margin: var(--space-2) 0 0; display: grid; gap: var(--space-1); }
.gx-doc-arg { display: grid; grid-template-columns: minmax(5rem, auto) 1fr; gap: 0 var(--space-3); padding: 0 var(--space-2); border-radius: var(--radius-sm); }
.gx-doc-arg-at { background: var(--accent-fill); }
.gx-doc-arg-name { font-family: var(--font-mono); color: var(--syn-name); display: flex; flex-direction: column; }
.gx-doc-arg-facts { font-family: var(--font-sans); color: var(--text-muted); font-size: var(--fs-micro); display: flex; gap: var(--space-2); flex-wrap: wrap; }
.gx-doc-arg-required { color: var(--danger); }
.gx-doc-arg-doc { margin: 0; color: var(--text); }
.gx-doc-arg-text { margin: 0; }
.gx-doc-options { display: flex; flex-wrap: wrap; gap: var(--space-1); margin-top: var(--space-1); }
.gx-doc-option { padding: 0 var(--space-1); border: 1px solid var(--border); border-radius: var(--radius-sm); }
/* 16px is the iOS focus-zoom threshold, a device fact rather than a scale rung. */
@media (hover: none) and (pointer: coarse) {
	.gx-win { font-size: 16px; }
	.gx-win-item { min-height: var(--hit); align-items: center; }
}
`;

function mountStyle(): void {
	if (document.getElementById('gx-win-style')) return;
	const style = document.createElement('style');
	style.id = 'gx-win-style';
	style.textContent = WINDOW_STYLE;
	document.head.appendChild(style);
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls: string, text?: string): HTMLElementTagNameMap[K] {
	const node = document.createElement(tag);
	node.className = cls;
	if (text !== undefined) node.textContent = text;
	return node;
}

/** One option's row: its name, its line, its kind. */
function row(o: Completion): HTMLElement {
	const doc = (o as GoofiCompletion).doc;
	const li = el('li', 'gx-win-item');
	li.setAttribute('role', 'option');
	li.append(
		el('span', `gx-win-label gx-win-${o.type ?? 'name'}`, o.displayLabel ?? o.label),
		el('span', 'gx-win-brief', doc ? brief(doc).replace(/`/g, '') : ''),
		el('span', 'gx-win-detail', o.detail ?? '')
	);
	return li;
}

export function completionWindow(host: HTMLElement, catalogue: () => ExprCatalogue, rule: WindowRule): Extension {
	return ViewPlugin.fromClass(
		class {
			private readonly dom = el('div', 'gx-win');
			private readonly doc = el('div', 'gx-win-doc');
			private readonly list = el('ul', 'gx-win-list');
			/** The options the rows stand for: kept while the completion catches up with a keystroke. */
			private options: readonly Completion[] = [];
			private selected: number | null = null;
			private raf = 0;
			private readonly sizes: ResizeObserver;
			private readonly place = (): void => {
				if (!this.raf) this.raf = requestAnimationFrame(() => ((this.raf = 0), this.fit()));
			};

			constructor(private readonly view: EditorView) {
				mountStyle();
				this.dom.dataset.testid = 'expr-window';
				this.dom.hidden = true;
				// A press in the window must not take the focus — and with it the window — off the field.
				this.dom.addEventListener('pointerdown', (e) => e.preventDefault());
				this.list.setAttribute('role', 'listbox');
				this.list.addEventListener('click', (e) => {
					const li = (e.target as HTMLElement).closest<HTMLElement>('.gx-win-item');
					if (!li) return;
					this.view.dispatch({ effects: setSelectedCompletion(Array.prototype.indexOf.call(this.list.children, li)) });
					acceptCompletion(this.view);
				});
				this.dom.append(this.doc, this.list);
				document.body.appendChild(this.dom);
				this.sizes = new ResizeObserver(this.place);
				this.sizes.observe(host);
				window.addEventListener('resize', this.place);
				window.visualViewport?.addEventListener('resize', this.place);
				document.addEventListener('scroll', this.place, true);
				this.update();
			}

			/** Every update, the one path: shown or not; the doc at the cursor; the rows; the place. */
			update(): void {
				const { state } = this.view;
				const shown = this.view.hasFocus && !rule.dismissed;
				this.dom.hidden = !shown;
				host.toggleAttribute('data-window-open', shown);
				if (!shown) return;
				const machine = !!catalogue().machine;
				const site = docSite(state, state.selection.main.head, machine);
				this.doc.replaceChildren(site ? renderBuiltin(site.fn, site.arg) : renderScope(machine));
				const pending = completionStatus(state) === 'pending';
				const options = pending ? this.options : currentCompletions(state);
				const selected = pending ? this.selected : selectedCompletionIndex(state);
				this.list.classList.toggle('gx-win-stale', pending);
				if (options !== this.options) {
					this.options = options;
					this.list.replaceChildren(...options.map(row));
				}
				this.selected = selected;
				Array.from(this.list.children).forEach((li, i) => {
					if (i === selected) li.setAttribute('aria-selected', 'true');
					else li.removeAttribute('aria-selected');
				});
				this.list.children[selected ?? -1]?.scrollIntoView({ block: 'nearest' });
				this.fit();
			}

			/** Under the field, its width, its top on the field's bottom edge; as tall as the viewport leaves. */
			private fit(): void {
				const rect = host.getBoundingClientRect();
				const room = overlayViewport().height - MARGIN - rect.bottom;
				this.dom.style.left = `${rect.left}px`;
				this.dom.style.top = `${rect.bottom}px`;
				this.dom.style.width = `${rect.width}px`;
				this.dom.style.maxHeight = `${Math.max(room, MIN_HEIGHT)}px`;
			}

			destroy(): void {
				if (this.raf) cancelAnimationFrame(this.raf);
				this.sizes.disconnect();
				window.removeEventListener('resize', this.place);
				window.visualViewport?.removeEventListener('resize', this.place);
				document.removeEventListener('scroll', this.place, true);
				host.removeAttribute('data-window-open');
				this.dom.remove();
			}
		}
	);
}
