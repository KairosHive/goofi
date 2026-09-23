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
			this.groups.set(group.id, { ...group, uid: previous?.uid ?? this.nextUid++, lines: group.text.split('\n').length });
		}
		// The eviction scan is O(groups); it runs only when the floor moved.
		if (batch.oldest > this.oldest || batch.reset) {
			this.oldest = batch.oldest;
			for (const [id, group] of this.groups) {
				if (group.seq < batch.oldest) this.groups.delete(id);
			}
		}
		this.cursor = batch.cursor;
		this.scheduleBump();
	}

	view(node: string | null, levels: Set<LogLevel>, query: string): ConsoleView {
		const words = query.toLocaleLowerCase().trim().split(/\s+/).filter(Boolean);
		const rows = [...this.groups.values()].filter((g) => {
			if (node !== null && g.node !== node) return false;
			if (!levels.has(g.level)) return false;
			const haystack = `${g.component} ${g.node ?? ''} ${g.stream ?? ''} ${g.text}`.toLocaleLowerCase();
			return words.every((word) => haystack.includes(word));
		});
		return { total: () => rows.length, get: (i) => rows[i] };
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
