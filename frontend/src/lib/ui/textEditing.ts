/** The one answer both keyboard scopes ask: does this keystroke belong to a text editor or to the
 * app? Duck-typed over the two fields it reads, so it needs no DOM. */
export function isTextEditingTarget(target: EventTarget | null): boolean {
	const el = target as (Partial<HTMLElement> & EventTarget) | null;
	if (!el) return false;
	if (el.isContentEditable === true) return true;
	const tag = el.tagName ?? '';
	// A slider, toggle or colour well takes no letters, so the app's keys stay the app's.
	if (tag === 'INPUT') return !NO_TEXT.has((el as Partial<HTMLInputElement>).type ?? 'text');
	return tag === 'TEXTAREA' || tag === 'SELECT';
}
const NO_TEXT = new Set(['range', 'checkbox', 'radio', 'button', 'submit', 'reset', 'color', 'file']);
