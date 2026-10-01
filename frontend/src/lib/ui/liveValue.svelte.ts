import { untrack } from 'svelte';

/** The echo-suppression decision: which value a live control should display. */
export function displayValue<T>(editing: boolean, source: T, local: T): T {
	return editing ? local : source;
}

/** Wire a control's edit buffer to a live source: `input` previews through `onInput`, `commit` sends
 * through `onChange`, and the source is hidden between `begin` and `end` and until it echoes. */
export function useLiveValue<T>(getSource: () => T, onChange: (v: T) => void, onInput?: (v: T) => void) {
	let editing = $state(false);
	let edit = $state<T>(getSource());
	// A commit the source has not answered yet: shown in its place until the source next moves.
	let pending = $derived((getSource(), false));
	// A preview moved the source, so a commit at that value is still an edit to record.
	let previewed = false;

	const value = $derived(displayValue(editing || pending, getSource(), edit));

	return {
		get value() {
			return value;
		},
		get editing() {
			return editing;
		},
		begin() {
			edit = value; // seed from what is shown so there's no flash to a stale edit
			editing = true;
		},
		input(v: T) {
			edit = v;
			if (onInput) {
				previewed = true;
				onInput(v);
			}
		},
		commit(v: T) {
			edit = v;
			// A focus and blur with nothing typed is no edit: it must not dirty the patch.
			if (!previewed && Object.is(v, untrack(getSource))) return;
			previewed = false;
			pending = true;
			onChange(v);
		},
		end() {
			editing = false;
		}
	};
}
