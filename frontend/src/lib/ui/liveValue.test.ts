import { describe, it, expect, vi } from 'vitest';
import { displayValue, useLiveValue, SEND_INTERVAL_MS } from './liveValue.svelte';

// `displayValue` is the ONE echo-suppression decision that every live control shares
// (spec §3): given whether the user is actively editing, the latest backend `source`, and
// the in-progress `local` edit — which value should the control show? Extracted pure so the
// "value jumps under the cursor" fix (ParamField:29-37, generalised) is unit-tested once and
// every control opts into it via `useLiveValue` rather than re-deriving (and re-breaking) it.
describe('displayValue', () => {
	it('follows the backend source while idle (not editing)', () => {
		// Idle → the control tracks the live value; a fresh backend echo is shown.
		expect(displayValue(false, 7, 3)).toBe(7);
	});

	it('suppresses the backend echo while editing (shows the local edit)', () => {
		// Editing → the user's in-progress value wins, so a backend echo cannot yank the
		// value out from under the cursor.
		expect(displayValue(true, 7, 3)).toBe(3);
	});

	it('works across the value kinds a control carries', () => {
		expect(displayValue(false, 'live', 'typed')).toBe('live');
		expect(displayValue(true, 'live', 'typed')).toBe('typed');
		expect(displayValue(false, true, false)).toBe(true);
		expect(displayValue(true, true, false)).toBe(false);
	});

	it('a mid-edit source change is ignored, then followed once the edit ends (the bug it fixes)', () => {
		// Start idle, showing the backend value.
		let source = 5;
		let local = source;
		expect(displayValue(false, source, local)).toBe(5);

		// User begins editing and types 6; meanwhile the backend echoes 7.
		local = 6;
		source = 7;
		// The display stays on the user's 6 — it does NOT jump to the echoed 7.
		expect(displayValue(true, source, local)).toBe(6);

		// On commit/blur the latch releases; the display resumes following the source.
		expect(displayValue(false, source, local)).toBe(7);
	});

	it('is pure — identical inputs yield identical output', () => {
		expect(displayValue(true, 2, 9)).toBe(displayValue(true, 2, 9));
	});
});

// `useLiveValue` is the ONE owner of a gesture's send rate: a pointer stream of 60-125 events/s
// reaches the backend as latest-wins sends, one per interval, and the gesture's end always lands.
describe('useLiveValue paces what a gesture sends', () => {
	it('N commits inside one interval send once, with the newest value; end() flushes', () => {
		vi.useFakeTimers();
		try {
			const sent: number[] = [];
			const live = useLiveValue<number>(
				() => 0,
				(v) => void sent.push(v)
			);
			live.begin();
			live.commit(1);
			live.commit(2);
			live.commit(3);
			expect(sent, 'the first goes at once, the rest wait').toEqual([1]);
			expect(live.value, 'the control shows the newest at once').toBe(3);
			vi.advanceTimersByTime(SEND_INTERVAL_MS);
			expect(sent).toEqual([1, 3]);
			live.commit(4);
			live.commit(5);
			live.end();
			expect(sent, 'the end of a gesture does not wait for the interval').toEqual([1, 3, 5]);
			vi.advanceTimersByTime(SEND_INTERVAL_MS * 2);
			expect(sent, 'and nothing is left behind').toEqual([1, 3, 5]);
		} finally {
			vi.useRealTimers();
		}
	});

	it('skips a value equal to the last one sent', () => {
		vi.useFakeTimers();
		try {
			const sent: number[] = [];
			const live = useLiveValue<number>(
				() => 0,
				(v) => void sent.push(v)
			);
			live.commit(1);
			live.commit(2);
			live.commit(1);
			live.end();
			expect(sent).toEqual([1]);
			live.begin();
			live.commit(1);
			live.end();
			expect(sent, 'a new gesture sends it again: another writer may have moved the source').toEqual([1, 1]);
		} finally {
			vi.useRealTimers();
		}
	});

	it('holds the next send until an in-flight op settles, and shows the edit until then', async () => {
		vi.useFakeTimers();
		try {
			const sent: number[] = [];
			let settle: () => void = () => {};
			const live = useLiveValue<number>(
				() => 0,
				(v) => {
					sent.push(v);
					return new Promise<void>((r) => (settle = r));
				}
			);
			live.commit(1);
			live.commit(2);
			live.end();
			vi.advanceTimersByTime(SEND_INTERVAL_MS * 3);
			expect(sent, 'the interval passed, but the first op has not answered').toEqual([1]);
			expect(live.value, 'the gesture ended, but the source has not caught up').toBe(2);
			settle();
			await vi.advanceTimersByTimeAsync(0);
			expect(sent).toEqual([1, 2]);
			settle();
			await vi.advanceTimersByTimeAsync(0);
			expect(live.value, 'answered: the source is what shows').toBe(0);
		} finally {
			vi.useRealTimers();
		}
	});
});
