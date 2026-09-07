import { describe, it, expect } from 'vitest';
import { undoKeyAction, escapeKeyAction, type UndoKeyEvent, type EscapeKeyEvent } from './shellKeys';

const ev = (over: Partial<UndoKeyEvent>): UndoKeyEvent => ({
	key: 'z',
	ctrlKey: false,
	metaKey: false,
	shiftKey: false,
	editing: false,
	...over
});

describe('undoKeyAction', () => {
	it('Ctrl+Z → undo', () => {
		expect(undoKeyAction(ev({ ctrlKey: true, key: 'z' }), false)).toBe('undo');
	});
	it('Cmd+Z → undo', () => {
		expect(undoKeyAction(ev({ metaKey: true, key: 'z' }), false)).toBe('undo');
	});
	it('Ctrl+Shift+Z → redo', () => {
		expect(undoKeyAction(ev({ ctrlKey: true, shiftKey: true, key: 'z' }), false)).toBe('redo');
	});
	it('Ctrl+Y → redo', () => {
		expect(undoKeyAction(ev({ ctrlKey: true, key: 'y' }), false)).toBe('redo');
	});
	it('plain z → none', () => {
		expect(undoKeyAction(ev({ key: 'z' }), false)).toBe('none');
	});
	// `editing` is answered by `isTextEditingTarget`, which since X covers a contenteditable too — the
	// expression editor's Ctrl+Z has to reach CodeMirror's history, not the graph's (D-X7).
	it('suppressed while typing in any text-editing surface', () => {
		expect(undoKeyAction(ev({ ctrlKey: true, key: 'z', editing: true }), false)).toBe('none');
	});
	it('suppressed while a modal is open', () => {
		expect(undoKeyAction(ev({ ctrlKey: true, key: 'z' }), true)).toBe('none');
	});
});

const esc = (over: Partial<EscapeKeyEvent> = {}): EscapeKeyEvent => ({
	key: 'Escape',
	editing: false,
	consumed: false,
	...over
});

describe('escapeKeyAction', () => {
	it('Escape under a maximized panel ends the maximize', () => {
		expect(escapeKeyAction(esc(), false, true)).toBe('exit-maximize');
	});
	it('does nothing with no panel maximized', () => {
		expect(escapeKeyAction(esc(), false, false)).toBe('none');
	});
	it('leaves every other key alone', () => {
		expect(escapeKeyAction(esc({ key: 'q' }), false, true)).toBe('none');
	});
	// The shell is Escape's LAST rung. A panel that closed its menu, cleared its selection or stepped
	// out of a sub-patch says so with `preventDefault`, and keeps its maximize.
	it('stands down for a nearer handler that already took the key', () => {
		expect(escapeKeyAction(esc({ consumed: true }), false, true)).toBe('none');
	});
	it('stands down while typing, where the field owns Escape', () => {
		expect(escapeKeyAction(esc({ editing: true }), false, true)).toBe('none');
	});
	it('stands down while a modal is open', () => {
		expect(escapeKeyAction(esc(), true, true)).toBe('none');
	});
});
