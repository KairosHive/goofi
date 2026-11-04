/** The mounted expression editor — one lazy CodeMirror chunk: a Python expression with goofi's
 *  completions in the window under the field. */
import { EditorState, Prec, type Extension } from '@codemirror/state';
import { EditorView, keymap, placeholder, tooltips, type KeyBinding } from '@codemirror/view';
import { LanguageSupport, syntaxHighlighting } from '@codemirror/language';
import { pythonLanguage } from '@codemirror/lang-python';
import { acceptCompletion, autocompletion, closeCompletion, completionKeymap, completionStatus, startCompletion } from '@codemirror/autocomplete';
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
import { setDiagnostics } from '@codemirror/lint';
import { MARGIN, overlayViewport } from 'panelty';
import { goofiLanguageData } from './complete';
import { completionWindow, type WindowRule } from './window';
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

/** The window's one rule: open while the field has focus, closed by Escape alone, back with
 *  Ctrl+Space or the next focus. The completion restarts whenever it closed with the field focused. */
function keepOpen(rule: WindowRule): Extension {
	return EditorView.updateListener.of((update) => {
		const { view } = update;
		if (update.focusChanged && !view.hasFocus) rule.dismissed = false;
		if (!view.hasFocus || rule.dismissed) return;
		if ((update.focusChanged || update.docChanged || update.selectionSet) && completionStatus(update.state) === null) {
			queueMicrotask(() => {
				if (view.hasFocus && !rule.dismissed && completionStatus(view.state) === null) startCompletion(view);
			});
		}
	});
}

export function createExprEditor(host: HTMLElement, opts: ExprEditorOptions): ExprEditorHandle {
	let committed = opts.doc;
	const commit = (view: EditorView): void => {
		const next = view.state.doc.toString();
		if (next === committed) return;
		committed = next;
		opts.onCommit(next);
	};
	const rule: WindowRule = { dismissed: false };
	let tabWaits = false;
	/* Tab takes the completion, the moment the list shows; while the list still catches up with the
	   last keystroke it takes the option the list settles on, through `acceptWhenSettled`, rather
	   than falling through and moving the focus. Escape falls THROUGH once the window is closed:
	   it is the app's then, and dismisses the pane. */
	const keys: KeyBinding[] = [
		{
			key: 'Escape',
			run: (view) => {
				if (rule.dismissed) return false;
				rule.dismissed = true;
				if (!closeCompletion(view)) view.dispatch({});
				return true;
			}
		},
		{
			key: 'Ctrl-Space',
			run: (view) => {
				rule.dismissed = false;
				return startCompletion(view);
			}
		},
		{ key: 'Tab', run: (view) => acceptCompletion(view) || (tabWaits = completionStatus(view.state) === 'pending') },
		{ key: 'Enter', run: (view) => (commit(view), true) }
	];
	const acceptWhenSettled = EditorView.updateListener.of((update) => {
		if (!tabWaits) return;
		const status = completionStatus(update.state);
		if (update.docChanged || status === null) tabWaits = false;
		else if (status === 'active') {
			tabWaits = false;
			queueMicrotask(() => acceptCompletion(update.view));
		}
	});
	const extensions: Extension[] = [
		// Python's own sources are folded into goofi's, so every option carries a doc.
		new LanguageSupport(pythonLanguage, [goofiLanguageData(opts.catalogue)]),
		syntaxHighlighting(exprHighlight),
		// CodeMirror's own popup stays hidden (theme.ts): the window renders what its state holds.
		autocompletion({ defaultKeymap: false, interactionDelay: 0 }),
		completionWindow(host, opts.catalogue, rule),
		keepOpen(rule),
		acceptWhenSettled,
		keymap.of(completionKeymap.filter((b) => b.key !== 'Enter')),
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
