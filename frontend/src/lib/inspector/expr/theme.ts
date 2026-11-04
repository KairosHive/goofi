/** The editor's look. CSS-in-JS because CodeMirror generates these class names and mounts the popup in
 *  `document.body`, out of reach of a Svelte-scoped rule; every value stays a `var(--…)`. */
import { HighlightStyle } from '@codemirror/language';
import { EditorView } from '@codemirror/view';
import { tags as t } from '@lezer/highlight';

/** Tag → ink. The role assignment is One Dark's; only the values are ours. */
export const EXPR_HIGHLIGHT_SPEC = [
	{ tag: t.keyword, color: 'var(--syn-keyword)' },
	{ tag: [t.name, t.deleted, t.character, t.propertyName, t.macroName], color: 'var(--syn-name)' },
	{ tag: [t.function(t.variableName), t.labelName], color: 'var(--syn-function)' },
	{ tag: [t.atom, t.bool, t.special(t.variableName), t.constant(t.name)], color: 'var(--syn-literal)' },
	{ tag: [t.definition(t.name), t.separator], color: 'var(--syn-punct)' },
	{
		tag: [t.typeName, t.className, t.number, t.changed, t.annotation, t.modifier, t.self, t.namespace],
		color: 'var(--syn-type)'
	},
	{ tag: [t.operator, t.operatorKeyword, t.escape, t.regexp, t.special(t.string)], color: 'var(--syn-operator)' },
	{ tag: [t.meta, t.comment], color: 'var(--syn-comment)', fontStyle: 'italic' },
	{ tag: [t.processingInstruction, t.string, t.inserted], color: 'var(--syn-string)' },
	{ tag: t.invalid, color: 'var(--danger)' }
];

export const exprHighlight = HighlightStyle.define(EXPR_HIGHLIGHT_SPEC);

export const EXPR_THEME_SPEC = {
	'&': {
		background: 'transparent',
		color: 'var(--text)',
		// The host element wears the app-wide focus ring; this would double it.
		'&.cm-focused': { outline: 'none' }
	},
	'.cm-scroller': { fontFamily: 'inherit', lineHeight: 'var(--lh-text)' },
	'.cm-content': { padding: 'var(--space-2) var(--space-4)', caretColor: 'var(--accent)' },
	'&.cm-focused .cm-cursor': { borderLeftColor: 'var(--accent)' },
	'.cm-selectionBackground, &.cm-focused .cm-selectionBackground, .cm-content ::selection': {
		background: 'var(--accent-fill)'
	},
	'.cm-placeholder': { color: 'var(--text-muted)' },

	/* The completion popup: CodeMirror copies the view's theme classes onto the container it creates
	   in `document.body`, which is what lets these rules reach it. */
	'.cm-tooltip': {
		background: 'var(--surface-2)',
		border: '1px solid var(--border)',
		borderRadius: 'var(--radius-sm)',
		boxShadow: 'var(--shadow-1)',
		color: 'var(--text)',
		zIndex: 'var(--z-menu)'
	},
	'.cm-tooltip.cm-tooltip-autocomplete > ul': {
		fontFamily: 'inherit',
		fontSize: 'var(--fs-small)',
		maxHeight: '14rem',
		'& > li': {
			display: 'flex',
			alignItems: 'center',
			gap: 'var(--space-4)',
			padding: 'var(--space-2) var(--space-4)'
		},
		'& > li[aria-selected]': { background: 'var(--accent-fill)', color: 'var(--text)' }
	},
	'.cm-completionLabel': { flex: '1' },
	'.cm-completionMatchedText': { textDecoration: 'none', color: 'var(--accent)' },
	'.cm-completionDetail': {
		fontStyle: 'normal',
		color: 'var(--text-muted)',
		marginLeft: '0',
		maxWidth: '55%',
		overflow: 'hidden',
		textOverflow: 'ellipsis',
		whiteSpace: 'nowrap'
	},
	'.cm-completionIcon': { color: 'var(--text-dim)', opacity: '1' },
	'.cm-tooltip.cm-tooltip-autocomplete': {
		minWidth: 'min(24rem, calc(100vw - 2 * var(--space-4)))',
		maxWidth: 'min(36rem, calc(100vw - 2 * var(--space-4)))'
	},

	/* The doc pane: in flow under the list, one window with it, instead of the base theme's box
	   floated beside. The content is `doc.ts`'s. */
	'.cm-tooltip.cm-completionInfo.cm-completionInfo-below': {
		position: 'static',
		width: 'auto',
		maxWidth: 'none',
		padding: '0',
		border: 'none',
		borderTop: '1px solid var(--border)',
		borderRadius: '0',
		boxShadow: 'none',
		whiteSpace: 'normal',
		maxHeight: '14rem',
		overflowY: 'auto',
		fontFamily: 'var(--font-sans)',
		fontSize: 'var(--fs-small)',
		lineHeight: 'var(--lh-text)'
	},
	'.gx-doc': { display: 'flex', flexDirection: 'column' },
	'.gx-doc-site': { padding: 'var(--space-2) var(--space-4)', borderBottom: '1px dashed var(--border)', background: 'var(--surface-1)' },
	'.gx-doc-own': { padding: 'var(--space-2) var(--space-4)' },
	'.gx-doc-sig': { display: 'block', fontFamily: 'var(--font-mono)', color: 'var(--syn-function)', fontWeight: '600' },
	'.gx-doc-text': { margin: 'var(--space-1) 0 0', color: 'var(--text)' },
	'.gx-doc-code, .gx-doc-option': { fontFamily: 'var(--font-mono)', color: 'var(--syn-string)' },
	'.gx-doc-args': { margin: 'var(--space-2) 0 0', display: 'grid', gap: 'var(--space-1)' },
	'.gx-doc-arg': { display: 'grid', gridTemplateColumns: 'minmax(5rem, auto) 1fr', gap: '0 var(--space-3)', padding: '0 var(--space-2)', borderRadius: 'var(--radius-sm)' },
	'.gx-doc-arg-at': { background: 'var(--accent-fill)' },
	'.gx-doc-arg-name': { fontFamily: 'var(--font-mono)', color: 'var(--syn-name)', display: 'flex', flexDirection: 'column' },
	'.gx-doc-arg-facts': { fontFamily: 'var(--font-sans)', color: 'var(--text-muted)', fontSize: 'var(--fs-micro)', display: 'flex', gap: 'var(--space-2)', flexWrap: 'wrap' },
	'.gx-doc-arg-required': { color: 'var(--danger)' },
	'.gx-doc-arg-doc': { margin: '0', color: 'var(--text)' },
	'.gx-doc-arg-text': { margin: '0' },
	'.gx-doc-options': { display: 'flex', flexWrap: 'wrap', gap: 'var(--space-1)', marginTop: 'var(--space-1)' },
	'.gx-doc-option': { padding: '0 var(--space-1)', border: '1px solid var(--border)', borderRadius: 'var(--radius-sm)' },

	/* `underline wavy` rather than the base theme's data-URI squiggle, whose colour is baked into the SVG. */
	'.cm-lintRange-error': {
		backgroundImage: 'none',
		textDecoration: 'underline wavy var(--danger)',
		textDecorationSkipInk: 'none'
	},
	'.cm-tooltip-lint': { fontFamily: 'inherit', fontSize: 'var(--fs-small)' },
	'.cm-diagnostic': { padding: 'var(--space-2) var(--space-4)', borderLeftColor: 'var(--danger)' },
	'.cm-diagnostic-error': { borderLeftColor: 'var(--danger)' },

	/* `16px` is the iOS focus-zoom threshold, a device fact rather than a scale rung. */
	'@media (hover: none) and (pointer: coarse)': {
		'.cm-tooltip.cm-tooltip-autocomplete > ul': { fontSize: '16px', maxHeight: '9rem' },
		'.cm-tooltip.cm-tooltip-autocomplete > ul > li': { minHeight: 'var(--hit)' },
		'.cm-tooltip.cm-completionInfo.cm-completionInfo-below': { fontSize: '16px', maxHeight: '9rem' }
	}
};

export const exprTheme = EditorView.theme(EXPR_THEME_SPEC);
