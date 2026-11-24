/** Where a menu drops from the control that opened it: under that control's left end. */
export function under(e: { currentTarget: EventTarget | null }): { x: number; y: number } {
	const box = (e.currentTarget as Element).getBoundingClientRect();
	return { x: box.left, y: box.bottom };
}
