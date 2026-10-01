/** Undo/redo as the MANAGER keeps it: every write is a step in this actor's history there, and
 * this store only mirrors what the replies say is on top. A transaction hands its calls one
 * step, which the manager merges into one entry under the transaction's label. */
import { getControl, historyFeed, type Control, type HistoryLabels, type Step } from '$lib/api/control';
import { asNavContext, captureNavContext, restoreNavContext } from './navContext';
import { pulseRestored } from './undoFlash';
import { graph, type GraphStore } from './graph.svelte';
import { notify } from './notify.svelte';

/** What `undo` and `redo` answer. */
interface FlipReply extends HistoryLabels {
	changed: boolean;
	context: unknown;
	stale: string | null;
}

export class HistoryStore {
	canUndo = $state(false);
	canRedo = $state(false);
	undoLabel = $state<string | null>(null);
	redoLabel = $state<string | null>(null);

	/** Re-entrancy guard: a held Ctrl+Z must not send a second flip before the first answers. */
	private replaying = false;
	private control: () => Control = getControl;
	private graph: () => GraphStore = graph;

	constructor() {
		historyFeed.labels = (h) => this.adopt(h);
		historyFeed.context = captureNavContext;
	}

	/** Test seam: flip against an injected control and the graph store that mirrors it. */
	configure(control: () => Control, graph: () => GraphStore): void {
		this.control = control;
		this.graph = graph;
	}

	/** Take the labels a reply or the hello carried. */
	adopt(h: HistoryLabels): void {
		this.undoLabel = h.undo;
		this.redoLabel = h.redo;
		this.canUndo = h.undo !== null;
		this.canRedo = h.redo !== null;
	}

	private async flip(op: 'undo' | 'redo'): Promise<void> {
		const verb = op === 'undo' ? 'Undo' : 'Redo';
		if (this.replaying) return;
		this.replaying = true;
		let reply: FlipReply;
		try {
			reply = await this.control().call<FlipReply>(op, {});
		} catch (e) {
			notify().failure(verb, e);
			return;
		} finally {
			this.replaying = false;
		}
		this.adopt(reply);
		if (reply.stale) notify().raise(`${verb}: “${reply.stale}” no longer applies and was dropped`);
		const ctx = reply.changed ? asNavContext(reply.context) : null;
		if (ctx) {
			await restoreNavContext(ctx);
			pulseRestored(ctx, this.graph());
		}
	}

	async undo(): Promise<void> {
		return this.flip('undo');
	}

	async redo(): Promise<void> {
		return this.flip('redo');
	}

	/** Every write inside `fn` is ONE step under `label`; a nested transaction rides the outer one. */
	/** Run `fn` with one step every call inside hands on, so the manager merges them under
	 * `label`. Given an enclosing `within`, the calls join that step instead. */
	transaction<T>(label: string, fn: (step: Step) => Promise<T>, within?: Step): Promise<T> {
		return fn(within ?? { group: `${Date.now()}-${Math.random().toString(36).slice(2)}`, label });
	}

	/** A new session or a wholesale load: the manager's history is empty, so this mirror is too. */
	reset(): void {
		this.adopt({ undo: null, redo: null });
	}
}

let _history: HistoryStore | null = null;
export function history(): HistoryStore {
	if (!_history) _history = new HistoryStore();
	return _history;
}
