import { untrack } from 'svelte';

/** Wire a control's edit buffer to a live source: `input` previews through `onInput`, `commit` sends
 * through `onChange`, and the source is hidden between `begin` and `end` and until it echoes. */
export function useLiveValue<T>(getSource: () => T, onChange: (v: T) => void, onInput?: (v: T) => void) {
	let editing = $state(false);
	let edit = $state<T>(getSource());
	// A commit the source has not answered yet: shown in its place until the source next moves.
	let pending = $derived((getSource(), false));
	// A preview moved the source, so a commit at that value is still an edit to record.
	let previewed = false;
	// An input the control has not committed: a release keeps showing it until the commit
	// arrives, because a range's change event follows its pointer-up.
	let dirty = false;
	let released = false;

	const value = $derived(editing || pending ? edit : getSource());

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
			released = false;
		},
		input(v: T) {
			edit = v;
			dirty = true;
			if (onInput) {
				previewed = true;
				onInput(v);
			}
		},
		commit(v: T) {
			edit = v;
			dirty = false;
			if (released) editing = false;
			// A focus and blur with nothing typed is no edit: it must not dirty the patch.
			if (!previewed && Object.is(v, untrack(getSource))) return;
			previewed = false;
			pending = true;
			onChange(v);
		},
		end() {
			released = true;
			if (!dirty) editing = false;
		}
	};
}
