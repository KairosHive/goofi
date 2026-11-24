import { describe, it, expect } from 'vitest';
import { shellKeyAction } from './shellKeys';

type Key = Parameters<typeof shellKeyAction>[0];
const ev = (over: Partial<Key>): Key => ({
	key: 'z',
	ctrlKey: false,
	metaKey: false,
	shiftKey: false,
	defaultPrevented: false,
	...over
});

describe('shellKeyAction: undo and redo', () => {
	it('Ctrl+Z → undo', () => {
		expect(shellKeyAction(ev({ ctrlKey: true, key: 'z' }), false, false)).toBe('undo');
	});
	it('Cmd+Z → undo', () => {
		expect(shellKeyAction(ev({ metaKey: true, key: 'z' }), false, false)).toBe('undo');
	});
	it('Ctrl+Shift+Z → redo', () => {
		expect(shellKeyAction(ev({ ctrlKey: true, shiftKey: true, key: 'z' }), false, false)).toBe('redo');
	});
	it('Ctrl+Y → redo', () => {
		expect(shellKeyAction(ev({ ctrlKey: true, key: 'y' }), false, false)).toBe('redo');
	});
	it('plain z → none', () => {
		expect(shellKeyAction(ev({ key: 'z' }), false, false)).toBe(null);
	});
	// The expression editor's Ctrl+Z has to reach CodeMirror's history, not the graph's (D-X7);
	// a text-editing target or an open modal is the stand-down.
	it('suppressed while typing or while a modal is open', () => {
		expect(shellKeyAction(ev({ ctrlKey: true, key: 'z' }), true, false)).toBe(null);
	});
});

const esc = (over: Partial<Key> = {}): Key => ev({ key: 'Escape', ...over });

describe('shellKeyAction: Escape', () => {
	it('Escape under a maximized panel ends the maximize', () => {
		expect(shellKeyAction(esc(), false, true)).toBe('exit-maximize');
	});
	it('does nothing with no panel maximized', () => {
		expect(shellKeyAction(esc(), false, false)).toBe(null);
	});
	it('leaves every other key alone', () => {
		expect(shellKeyAction(esc({ key: 'q' }), false, true)).toBe(null);
	});
	// The shell is Escape's last rung. A panel that took the key says so with `preventDefault`.
	it('stands down for a nearer handler that already took the key', () => {
		expect(shellKeyAction(esc({ defaultPrevented: true }), false, true)).toBe(null);
	});
	it('stands down while typing or while a modal is open', () => {
		expect(shellKeyAction(esc(), true, true)).toBe(null);
	});
});
