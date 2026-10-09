/** Read-only projection of the backend's log groups. Filters never change counts. */
export type LogLevel = 'info' | 'warning' | 'error';

export interface LogGroup {
	id: number;
	seq: number;
	count: number;
	ts: number;
	component: string;
	node: string | null;
	level: LogLevel;
	stream: string | null;
	text: string;
}

export interface LogBatch {
	cursor: number;
	oldest: number;
	reset: boolean;
	groups: (Pick<LogGroup, 'id' | 'seq' | 'count' | 'ts'> & Partial<LogGroup>)[];
}

export interface ConsoleEntry extends LogGroup {
	uid: number;
	lines: number;
	/** The searchable text, lowercased once on arrival: what a query's words are matched against. */
	haystack: string;
}

export interface ConsoleView {
	total(): number;
	get(i: number): ConsoleEntry;
}

export class ConsoleStore {
	private groups = new Map<number, ConsoleEntry>();
	private cursor = -1;
	private oldest = -1;
	private nextUid = 0;
	version = $state(0);
	/** Bumped when the floor moved and groups left: what a reader keyed by uid prunes on. */
	evicted = $state(0);
	private scheduled = false;

	apply(batch: LogBatch): void {
		if (!batch.reset && batch.cursor <= this.cursor) return;
		if (batch.reset) this.groups.clear();
		for (const update of batch.groups) {
			const previous = this.groups.get(update.id);
			if (previous && previous.seq >= update.seq) continue;
			if (!previous && update.text === undefined) continue;
			const group = { ...previous, ...update } as LogGroup;
			this.groups.delete(group.id);
			const haystack = `${group.component} ${group.node ?? ''} ${group.stream ?? ''} ${group.text}`.toLocaleLowerCase();
			this.groups.set(group.id, { ...group, uid: previous?.uid ?? this.nextUid++, lines: group.text.split('\n').length, haystack });
		}
		// The eviction scan is O(groups); it runs only when the floor moved.
		if (batch.oldest > this.oldest || batch.reset) {
			this.oldest = batch.oldest;
			const held = this.groups.size;
			for (const [id, group] of this.groups) {
				if (group.seq < batch.oldest) this.groups.delete(id);
			}
			if (this.groups.size < held || batch.reset) this.evicted += 1;
		}
		this.cursor = batch.cursor;
		this.scheduleBump();
	}

	view(node: string | null, levels: Set<LogLevel>, query: string): ConsoleView {
		const words = query.toLocaleLowerCase().trim().split(/\s+/).filter(Boolean);
		const rows = [...this.groups.values()].filter(
			(g) => (node === null || g.node === node) && levels.has(g.level) && words.every((word) => g.haystack.includes(word))
		);
		return { total: () => rows.length, get: (i) => rows[i] };
	}

	/** The log's position now, for `since`. */
	mark(): number {
		return this.cursor;
	}

	/** Every group written or repeated after `mark`, oldest first: what an operation started at
	 * `mark` has said so far. */
	since(mark: number): ConsoleEntry[] {
		return [...this.groups.values()].filter((g) => g.seq > mark);
	}

	private scheduleBump(): void {
		if (this.scheduled) return;
		this.scheduled = true;
		const run = () => { this.scheduled = false; this.version += 1; };
		if (typeof requestAnimationFrame === 'function') requestAnimationFrame(run);
		else queueMicrotask(run);
	}
}

let store: ConsoleStore | undefined;
export function consoleStore(): ConsoleStore { return store ??= new ConsoleStore(); }
