/** The echo-suppression decision: which value a live control should display. */
export function displayValue<T>(editing: boolean, source: T, local: T): T {
	return editing ? local : source;
}

/** The live-value handle a control drives from its DOM events: `input` buffers, `commit` sends to
 * the backend, and a backend echo is suppressed between `begin` and `end`. */
export interface LiveValue<T> {
	readonly value: T;
	readonly editing: boolean;
	begin(): void;
	input(v: T): void;
	commit(v: T): void;
	end(): void;
}

/** The pace one gesture sends at: newest value wins, at most one send per interval. */
export const SEND_INTERVAL_MS = 40;

/** Wire a control's local edit buffer to a live backend source. `onChange` may answer a promise:
 * the edit then shows until it settles, and the next send waits for it — one op per control. */
export function useLiveValue<T>(getSource: () => T, onChange: (v: T) => unknown): LiveValue<T> {
	let editing = $state(false);
	let edit = $state<T>(getSource());
	// From a commit until its send is answered: the source has not caught up before the reply.
	let unsettled = $state(false);

	const value = $derived(displayValue(editing || unsettled, getSource(), edit));

	let pending: { v: T } | null = null;
	let sent: { v: T } | null = null;
	let sentAt = -Infinity;
	let timer: ReturnType<typeof setTimeout> | null = null;
	let inFlight = false;

	function flush(now: boolean): void {
		if (pending && !inFlight) {
			const wait = sentAt + SEND_INTERVAL_MS - Date.now();
			if (!now && wait > 0) {
				timer ??= setTimeout(() => ((timer = null), flush(true)), wait);
				return;
			}
			if (timer) clearTimeout(timer);
			timer = null;
			const { v } = pending;
			pending = null;
			if (!sent || !Object.is(sent.v, v)) {
				sent = { v };
				sentAt = Date.now();
				const r = onChange(v);
				if (r instanceof Promise) {
					inFlight = true;
					r.finally(() => ((inFlight = false), flush(false))).catch(() => {});
				}
			}
		}
		unsettled = inFlight || pending !== null;
	}

	return {
		get value() {
			return value;
		},
		get editing() {
			return editing;
		},
		begin() {
			edit = value; // seed from what is shown — a value still on its way included — so there's no flash
			sent = null; // another writer may have moved the source since, so the last send says nothing
			editing = true;
		},
		input(v: T) {
			edit = v;
		},
		commit(v: T) {
			edit = v;
			pending = { v };
			unsettled = true;
			flush(false);
		},
		end() {
			editing = false;
			flush(true);
		}
	};
}
