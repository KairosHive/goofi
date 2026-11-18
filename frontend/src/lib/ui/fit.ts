import type { ActionReturn } from 'svelte/action';

/** Show the parts of `el` that fit in its height, in their order, and hide the others whole: a
 * part is a direct child marked `data-fit`, and the last ones go first. `onChange` hears the
 * `data-fit` names of the hidden parts, so the owner can offer them elsewhere, in a menu. */
export function fit(el: HTMLElement, onChange: (hidden: string[]) => void): ActionReturn<(hidden: string[]) => void> {
	let said = '';
	const measure = (): void => {
		const parts = [...el.querySelectorAll<HTMLElement>(':scope > [data-fit]')];
		// By its style, which a part's own `display` cannot outrank as it does the `hidden` attribute.
		for (const part of parts) part.style.display = '';
		for (let i = parts.length - 1; i >= 0 && el.scrollHeight > el.clientHeight; i--) parts[i].style.display = 'none';
		const hidden = parts.filter((part) => part.style.display === 'none').map((part) => part.dataset.fit!);
		if (hidden.join() !== said) onChange(hidden);
		said = hidden.join();
	};
	// The room moves with the element's box, what it takes with what is inside it. A part's own
	// style is an attribute, which is not watched, so a measure never calls for another.
	const box = new ResizeObserver(measure);
	const inside = new MutationObserver(measure);
	box.observe(el);
	inside.observe(el, { childList: true, subtree: true, characterData: true });
	return {
		update: (next) => void (onChange = next),
		destroy: () => {
			box.disconnect();
			inside.disconnect();
		}
	};
}
