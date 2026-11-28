import { describe, it, expect } from 'vitest';
import { EditorState } from '@codemirror/state';
import { singleLineExpression } from './singleLine';

/* The inline param field stays one line. The keymap binds Enter to commit; this is the backstop
 * for a paste, a drop or a programmatic set, and it flattens a newline to a space. */

const state = (doc: string) => EditorState.create({ doc, extensions: [singleLineExpression] });

describe('single-line mode rejects newlines', () => {
	it('flattens a pasted multi-line expression to one line', () => {
		const tr = state("nd('a')").update({ changes: { from: 7, insert: "\n+ 1\n* 2" } });
		expect(tr.state.doc.lines).toBe(1);
		expect(tr.state.doc.toString()).toBe("nd('a') + 1 * 2");
	});

	// A newline is one character and so is the space replacing it, so the caret the transaction asked
	// for is still the caret it gets — no off-by-one after a paste.
	it('keeps the selection the transaction asked for', () => {
		const tr = state('').update({ changes: { from: 0, insert: 'a\nb' }, selection: { anchor: 3 } });
		expect(tr.state.doc.toString()).toBe('a b');
		expect(tr.state.selection.main.head).toBe(3);
	});

	it('leaves a single-line change completely alone', () => {
		const tr = state('a').update({ changes: { from: 1, insert: ' + 1' } });
		expect(tr.state.doc.toString()).toBe('a + 1');
	});

	// Replacing a range, not just inserting: the flattening must rebuild every change in the set.
	it('flattens a replacement of existing text', () => {
		const tr = state('abc').update({ changes: { from: 1, to: 2, insert: 'X\nY' } });
		expect(tr.state.doc.toString()).toBe('aX Yc');
	});
});
