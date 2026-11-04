/** The completion window: an extension of the code field, under it at its width, open while the
 *  field has focus. The doc of what the cursor sits in heads it; the options CodeMirror's
 *  completion holds follow, selected and accepted through CodeMirror's own state. */
import { EditorView, ViewPlugin, type ViewUpdate } from '@codemirror/view';
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
.gx-win { position: fixed; box-sizing: border-box; display: flex; flex-direction: column; overflow: hidden;
	background: var(--surface-1); border: 1px solid var(--border); border-top: none;
	border-radius: 0 0 var(--radius-sm) var(--radius-sm); box-shadow: var(--shadow-1); color: var(--text);
	font-family: var(--font-sans); font-size: var(--fs-small); line-height: var(--lh-text); z-index: var(--z-menu); }
.gx-win[hidden] { display: none; }
/* The doc shows whole, as far as the window goes, and scrolls only once it does not fit; the list
   under it keeps three rows at the least and takes the rest. */
.gx-win-doc { flex: 0 1 auto; min-height: 0; overflow-y: auto; padding: var(--space-4) 0; background: var(--surface-2); border-bottom: 1px solid var(--border); }
/* The list is a table: a name column, a line column that gives way, a kind column; the rows subgrid
   onto it, so every column lines up down the list and a row still paints its selection. */
.gx-win-list { flex: 1 1 auto; min-height: calc(3 * (var(--lh-text) * 1em + 2 * var(--space-1))); overflow-y: auto; margin: 0; padding: var(--space-4) 0 0; list-style: none;
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
.gx-doc { display: flex; flex-direction: column; padding: 0 var(--space-4); }
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

export function completionWindow(host: HTMLElement, catalogue: () => ExprCatalogue, rule: WindowRule): Extension {
	return ViewPlugin.fromClass(
		class {
			readonly dom: HTMLElement;
			readonly doc: HTMLElement;
			readonly list: HTMLElement;
			private docKey = '';
			private options: readonly Completion[] = [];
			private selected: number | null = null;
			private shown = false;
			private raf = 0;
			private readonly sizes: ResizeObserver;
			private readonly place = (): void => {
				if (this.raf) return;
				this.raf = requestAnimationFrame(() => {
					this.raf = 0;
					if (this.shown) this.fit();
				});
			};

			constructor(private readonly view: EditorView) {
				mountStyle();
				this.dom = el('div', 'gx-win');
				this.dom.dataset.testid = 'expr-window';
				this.dom.hidden = true;
				// A press in the window must not take the focus — and with it the window — off the field.
				this.dom.addEventListener('pointerdown', (e) => e.preventDefault());
				this.doc = el('div', 'gx-win-doc');
				this.list = el('ul', 'gx-win-list');
				this.list.setAttribute('role', 'listbox');
				this.list.addEventListener('click', (e) => {
					const row = (e.target as HTMLElement).closest<HTMLElement>('.gx-win-item');
					if (!row || !row.parentElement) return;
					const index = Array.prototype.indexOf.call(row.parentElement.children, row);
					this.view.dispatch({ effects: setSelectedCompletion(index) });
					acceptCompletion(this.view);
				});
				this.dom.append(this.doc, this.list);
				document.body.appendChild(this.dom);
				this.sizes = new ResizeObserver(this.place);
				this.sizes.observe(host);
				window.addEventListener('resize', this.place);
				window.visualViewport?.addEventListener('resize', this.place);
				document.addEventListener('scroll', this.place, true);
				this.render();
			}

			update(_update: ViewUpdate): void {
				this.render();
			}

			render(): void {
				const { view } = this;
				const shown = view.hasFocus && !rule.dismissed;
				if (shown !== this.shown) {
					this.shown = shown;
					this.dom.hidden = !shown;
					host.toggleAttribute('data-window-open', shown);
				}
				if (!shown) return;
				this.renderDoc();
				this.renderList();
				this.fit();
			}

			/** The doc of where the cursor sits, rebuilt only when that moved to another site. */
			private renderDoc(): void {
				const { state } = this.view;
				const cat = catalogue();
				const machine = !!cat.machine;
				const site = docSite(state, state.selection.main.head, machine);
				const key = site ? `${site.fn.name}:${site.arg?.name ?? ''}` : `scope:${machine}`;
				if (key === this.docKey) return;
				this.docKey = key;
				this.doc.replaceChildren(site ? renderBuiltin(site.fn, site.arg) : renderScope(machine));
			}

			/** The options as CodeMirror holds them; a list still catching up keeps the last one, dimmed. */
			private renderList(): void {
				const { state } = this.view;
				const pending = completionStatus(state) === 'pending';
				const options = pending ? this.options : currentCompletions(state);
				const selected = pending ? this.selected : selectedCompletionIndex(state);
				this.list.classList.toggle('gx-win-stale', pending);
				if (options !== this.options) {
					this.options = options;
					this.selected = null;
					this.list.replaceChildren(
						...options.map((o) => {
							const row = el('li', 'gx-win-item');
							row.setAttribute('role', 'option');
							const doc = (o as GoofiCompletion).doc;
							row.appendChild(el('span', `gx-win-label gx-win-${o.type ?? 'name'}`, o.displayLabel ?? o.label));
							row.appendChild(el('span', 'gx-win-brief', doc ? brief(doc).replace(/`/g, '') : ''));
							row.appendChild(el('span', 'gx-win-detail', o.detail ?? ''));
							return row;
						})
					);
				}
				if (selected !== this.selected) {
					this.selected = selected;
					Array.from(this.list.children).forEach((row, i) => {
						if (i === selected) {
							row.setAttribute('aria-selected', 'true');
							row.scrollIntoView({ block: 'nearest' });
						} else row.removeAttribute('aria-selected');
					});
				}
			}

			/** Under the field, its width, its top on the field's bottom edge; as tall as the viewport leaves. */
			private fit(): void {
				const rect = host.getBoundingClientRect();
				const vp = overlayViewport();
				const room = vp.height - MARGIN - rect.bottom;
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
