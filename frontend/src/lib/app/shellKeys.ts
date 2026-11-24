/** The shell's chords once nothing nearer took the key: Ctrl/Cmd+Z undoes, Ctrl/Cmd+Shift+Z and
 * Ctrl+Y redo, and Escape ends a panel maximize. `standdown` is a modal or a text-editing target. */
export function shellKeyAction(
	e: Pick<KeyboardEvent, 'key' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'defaultPrevented'>,
	standdown: boolean,
	maximized: boolean
): 'undo' | 'redo' | 'exit-maximize' | null {
	if (standdown) return null;
	if (e.key === 'Escape') return maximized && !e.defaultPrevented ? 'exit-maximize' : null;
	if (!e.ctrlKey && !e.metaKey) return null;
	const key = e.key.toLowerCase();
	if (key === 'z') return e.shiftKey ? 'redo' : 'undo';
	return key === 'y' && !e.shiftKey ? 'redo' : null;
}
