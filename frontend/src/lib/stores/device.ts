/** The soft keyboard's overlap of the layout viewport, published as `--kb-inset` so `$lib/ui`
 * primitives can read it without importing a store. */

/** Pixels the soft keyboard overlaps the layout viewport (never negative). */
export function kbInset(viewportHeight: number, innerHeight: number): number {
	return Math.max(0, innerHeight - viewportHeight);
}

/** Keep `--kb-inset` current; returns the listener removal. */
export function trackKeyboardInset(): () => void {
	const vv = window.visualViewport;
	const measure = (): void => {
		document.documentElement.style.setProperty('--kb-inset', `${vv ? kbInset(vv.height, window.innerHeight) : 0}px`);
	};
	// The layout viewport is the reference the overlap is measured against, so it moves it too.
	vv?.addEventListener('resize', measure);
	window.addEventListener('resize', measure);
	measure();
	return () => {
		vv?.removeEventListener('resize', measure);
		window.removeEventListener('resize', measure);
	};
}
