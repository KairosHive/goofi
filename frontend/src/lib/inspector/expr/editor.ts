/** The mounted expression editor — one lazy CodeMirror chunk: a Python expression with goofi's
 *  completions. */
import { EditorState, Prec, type Extension } from '@codemirror/state';
import { EditorView, keymap, placeholder, tooltips, type KeyBinding } from '@codemirror/view';
import { syntaxHighlighting } from '@codemirror/language';
import { python } from '@codemirror/lang-python';
import { acceptCompletion, autocompletion, closeCompletion } from '@codemirror/autocomplete';
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { setDiagnostics } from '@codemirror/lint';
import { MARGIN, overlayViewport } from 'panelty';
import { goofiLanguageData } from './complete';
import { expressionDiagnostics } from './diagnostics';
import { singleLineExpression } from './singleLine';
import { exprHighlight, exprTheme } from './theme';
import type { ExprCatalogue } from './catalogue';

export interface ExprEditorOptions {
	doc: string;
	/** Read at the moment a completion is asked for. */
	catalogue: () => ExprCatalogue;
	/** Enter, or blur with a changed document. */
	onCommit: (value: string) => void;
	/** The backend's last compile/eval failure for this param, or null. */
	error: string | null;
	placeholder?: string;
	attributes: Record<string, string>;
}

export interface ExprEditorHandle {
	/** Adopt an externally changed source. A no-op while the user is typing into it. */
	setValue(next: string): void;
	setError(error: string | null): void;
	destroy(): void;
}

export function createExprEditor(host: HTMLElement, opts: ExprEditorOptions): ExprEditorHandle {
	let committed = opts.doc;
	const commit = (view: EditorView): void => {
		const next = view.state.doc.toString();
		if (next === committed) return;
		committed = next;
		opts.onCommit(next);
	};
	/* Escape must fall THROUGH once there is no popup: it is the app's, and it dismisses the auto
	   inspector pane. */
	const keys: KeyBinding[] = [
		{ key: 'Escape', run: (view) => closeCompletion(view) },
		{ key: 'Enter', run: (view) => acceptCompletion(view) || (commit(view), true) }
	];
	const extensions: Extension[] = [
		python(),
		goofiLanguageData(opts.catalogue),
		syntaxHighlighting(exprHighlight),
		autocompletion(),
		history(),
		exprTheme,
		/* Parented to `document.body` because an inspector panel clips its overflow, and sized against
		   `overlayViewport()`: CodeMirror's default `innerHeight` would park the list under the keyboard. */
		tooltips({
			parent: document.body,
			position: 'fixed',
			tooltipSpace: () => {
				const vp = overlayViewport();
				return { top: MARGIN, left: MARGIN, bottom: vp.height - MARGIN, right: vp.width - MARGIN };
			}
		}),
		EditorView.contentAttributes.of(opts.attributes),
		Prec.high(keymap.of(keys)),
		keymap.of([...historyKeymap, ...defaultKeymap]),
		EditorView.domEventHandlers({
			// The completion popup does not blur the editor, so accepting an option never races this.
			blur: (_e, view) => {
				commit(view);
				return false;
			}
		}),
		singleLineExpression
	];
	if (opts.placeholder) extensions.push(placeholder(opts.placeholder));
	const view = new EditorView({ state: EditorState.create({ doc: opts.doc, extensions }), parent: host });
	const setError = (error: string | null): void => {
		view.dispatch(setDiagnostics(view.state, expressionDiagnostics(error, view.state.doc)));
	};
	setError(opts.error);
	return {
		setValue: (next) => {
			// A live echo must not yank the document from under live typing; the committed value is
			// left alone, so the local text still commits on blur.
			if (next !== view.state.doc.toString()) {
				if (view.hasFocus) return;
				view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: next } });
			}
			committed = next;
		},
		setError,
		destroy: () => view.destroy()
	};
}
