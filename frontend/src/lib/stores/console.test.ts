import { describe, it, expect } from 'vitest';
import { ConsoleStore, type LogGroup } from './console.svelte';

const levels = new Set(['info', 'warning', 'error'] as const);
function group(id: number, text: string, node: string | null = null): LogGroup {
	return { id, seq: id, count: 1, ts: id, text, node, component: 'signal', level: 'info', stream: 'stdout' };
}
const rows = (s: ConsoleStore, node: string | null = null, query = '') => {
	const v = s.view(node, levels, query);
	return Array.from({ length: v.total() }, (_, i) => v.get(i));
};

describe('console log replica', () => {
	it('moves an interleaved repeat to the bottom without changing its identity', () => {
		const s = new ConsoleStore();
		s.apply({ reset: true, cursor: 2, oldest: 1, groups: [group(1, 'first', 'a'), group(2, 'second', 'b')] });
		s.apply({ reset: false, cursor: 3, oldest: 2, groups: [{ id: 1, seq: 3, count: 2, ts: 3 }] });
		expect(rows(s).map((g) => [g.id, g.text, g.count])).toEqual([[2, 'second', 1], [1, 'first', 2]]);
		expect(rows(s, 'a')[0].count).toBe(2);
		expect(rows(s, null, 'FIRST stdout').map((g) => g.id)).toEqual([1]);
		// A repeated packet must not double the count or move an old group again.
		s.apply({ reset: false, cursor: 3, oldest: 2, groups: [{ id: 1, seq: 3, count: 2, ts: 3 }] });
		expect(rows(s)[1].count).toBe(2);
	});

	it('replaces history on reconnect, applies eviction and clears from the server', () => {
		const s = new ConsoleStore();
		s.apply({ reset: true, cursor: 2, oldest: 1, groups: [group(1, 'old'), group(2, 'kept')] });
		s.apply({ reset: false, cursor: 3, oldest: 2, groups: [group(3, 'new')] });
		expect(rows(s).map((g) => g.text)).toEqual(['kept', 'new']);
		s.apply({ reset: true, cursor: 1, oldest: 1, groups: [group(1, 'other server')] });
		expect(rows(s).map((g) => g.text)).toEqual(['other server']);
		s.apply({ reset: false, cursor: 2, oldest: 3, groups: [] });
		expect(rows(s)).toEqual([]);
	});

	it('filters severity without regrouping history', () => {
		const s = new ConsoleStore();
		s.apply({ reset: true, cursor: 2, oldest: 1, groups: [group(1, 'info'), { ...group(2, 'warning'), level: 'warning', count: 4 }] });
		const v = s.view(null, new Set(['warning']), '');
		expect(v.total()).toBe(1);
		expect(v.get(0).count).toBe(4);
	});
});
