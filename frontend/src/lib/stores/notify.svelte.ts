/** What a rejection says: its message, or the value itself. */
export function errorText(e: unknown): string {
	return (e as Error)?.message ?? String(e);
}

/** The app's one transient alarm channel — not a queue: the latest message wins. */
export class NotifyStore {
	message = $state<string | null>(null);

	raise(text: string): void {
		this.message = text;
	}

	/** Raise a verb plus whatever an RPC rejection threw. */
	failure(verb: string, e: unknown): void {
		this.raise(`${verb} failed: ${errorText(e)}`);
	}

	/** Dismiss — a click on the toast, or its own timeout. */
	clear(): void {
		this.message = null;
	}
}

let instance: NotifyStore | null = null;

/** The app-wide alarm singleton. */
export function notify(): NotifyStore {
	if (!instance) instance = new NotifyStore();
	return instance;
}
