/** Test double for `Control`: it records every `call`, and a test drives the event stream with `emit`.
 * It keeps the manager's half of the history too — one labelled entry per write, merged under a
 * group token — so a store test sees the undo and redo the real manager would answer. */
import { historyFeed, type Control, type ControlEvent, type Step } from '$lib/api/control';
import { OP_KINDS, type OpName } from '$lib/api/ops';

interface Entry {
	label: string;
	group?: string;
	context: unknown;
}

export class FakeControl implements Control {
	/** Fixed stand-in for the tab's minted actor id. */
	readonly actor = 'fake-actor';
	private calls: Array<{ op: OpName; payload: Record<string, unknown> }> = [];
	/** Every preview sent, in order, with the key it would coalesce on. */
	previews: Array<{ key: string; op: OpName; payload: Record<string, unknown> }> = [];
	private listeners = new Set<(ev: ControlEvent) => void>();
	private connectListeners = new Set<(c: boolean) => void>();
	private results = new Map<string, unknown>();
	private failing = new Set<string>();
	// Starts connected, like the real ControlClient; a boot test asks for `{ connected: false }`.
	private _connected: boolean;
	/** The manager's history for this actor: what an undo takes back, newest last. */
	undoStack: Entry[] = [];
	redoStack: Entry[] = [];
	constructor({ connected = true }: { connected?: boolean } = {}) {
		this._connected = connected;
	}

	/** The labels the manager would put on a reply. */
	labels(): { undo: string | null; redo: string | null } {
		return {
			undo: this.undoStack.at(-1)?.label ?? null,
			redo: this.redoStack.at(-1)?.label ?? null
		};
	}

	/** A write lands as one entry, or merges into the entry sharing its group token. */
	private step(op: OpName, step?: Step): void {
		const top = this.undoStack.at(-1);
		this.redoStack = [];
		if (step && top?.group === step.group) return;
		this.undoStack.push({ label: step?.label ?? op, group: step?.group, context: historyFeed.context() });
	}

	/** Make `call(op, …)` resolve to `value` (e.g. `add_node` → a display name). */
	setCallResult(op: OpName, value: unknown): void {
		this.results.set(op, value);
	}

	/** Make the NEXT `call(op, …)` reject once — simulates a dispatch/transport error. */
	failNext(op: OpName): void {
		this.failing.add(op);
	}

	call<T = unknown>(op: OpName, payload: Record<string, unknown> = {}, step?: Step): Promise<T> {
		this.calls.push({ op, payload });
		if (this.failing.has(op)) {
			this.failing.delete(op);
			return Promise.reject(new Error(`fake control: ${op} failed`));
		}
		if (op === 'undo' || op === 'redo') {
			const [from, to] = op === 'undo' ? [this.undoStack, this.redoStack] : [this.redoStack, this.undoStack];
			const entry = from.pop();
			if (entry) to.push(entry);
			const reply = { changed: !!entry, context: entry?.context ?? null, stale: null, ...this.labels() };
			historyFeed.labels(this.labels());
			return Promise.resolve(reply as T);
		}
		if (OP_KINDS[op] === 'write' || op === 'compound') {
			this.step(op, step);
			historyFeed.labels(this.labels());
		}
		return Promise.resolve(this.results.get(op) as T);
	}

	preview(key: string, op: OpName, payload: Record<string, unknown>): void {
		this.previews.push({ key, op, payload });
	}

	on(fn: (ev: ControlEvent) => void): () => void {
		this.listeners.add(fn);
		return () => this.listeners.delete(fn);
	}

	onConnect(fn: (c: boolean) => void): () => void {
		this.connectListeners.add(fn);
		fn(this._connected); // fire immediately, like the real ControlClient
		return () => this.connectListeners.delete(fn);
	}

	/** Synchronously fan an event out to every `on` listener. */
	emit(ev: ControlEvent): void {
		if (ev.event === 'hello' || ev.event === 'graph_replaced') {
			ev = {
				...ev,
				payload: {
					...ev.payload,
					record: ev.payload.record ?? { running: false, folder: null, elapsed: null, streams: [], error: null }
				}
			};
		}
		for (const fn of this.listeners) fn(ev);
	}

	/** Drive connection listeners (e.g. simulate a reconnect: setConnected(false) then true). */
	setConnected(c: boolean): void {
		this._connected = c;
		for (const fn of this.connectListeners) fn(c);
	}

	recordedCalls(): Array<{ op: string; payload: Record<string, unknown> }> {
		return this.calls;
	}
}
