/** The global undo/redo chords: Ctrl/Cmd+Z undoes, Ctrl/Cmd+Shift+Z and Ctrl+Y redo. */
export interface UndoKeyEvent {
	key: string;
	ctrlKey: boolean;
	metaKey: boolean;
	shiftKey: boolean;
	/** Whether the event landed in a text-editing surface, a contenteditable included. */
	editing: boolean;
}

export type UndoKeyResult = 'undo' | 'redo' | 'none';

export function undoKeyAction(e: UndoKeyEvent, modalOpen: boolean): UndoKeyResult {
	if (modalOpen) return 'none';
	if (e.editing) return 'none';
	const meta = e.ctrlKey || e.metaKey;
	if (!meta) return 'none';
	const key = e.key.toLowerCase();
	if (key === 'z') return e.shiftKey ? 'redo' : 'undo';
	if (key === 'y' && !e.shiftKey) return 'redo';
	return 'none';
}

/** Escape at the shell: the key nothing nearer has taken. */
export interface EscapeKeyEvent {
	key: string;
	/** Whether the event landed in a text-editing surface, whose own Escape is the way out of it. */
	editing: boolean;
	/** Whether a nearer handler already acted on it — `defaultPrevented` is how they all say so. */
	consumed: boolean;
}

export type EscapeKeyResult = 'exit-maximize' | 'none';

/** Escape's last rung: it ends a panel maximize once nothing nearer has claimed the key. */
export function escapeKeyAction(
	e: EscapeKeyEvent,
	modalOpen: boolean,
	maximized: boolean
): EscapeKeyResult {
	if (modalOpen) return 'none';
	if (e.editing || e.consumed) return 'none';
	if (e.key !== 'Escape' || !maximized) return 'none';
	return 'exit-maximize';
}
