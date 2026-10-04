import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { DataFrame } from '$lib/codec/decode';

/** Minimal Worker stand-in: records postMessage, emits inbound. */
class MockWorker {
	static instances: MockWorker[] = [];
	posted: unknown[] = [];
	private listeners: ((e: MessageEvent) => void)[] = [];
	constructor() {
		MockWorker.instances.push(this);
	}
	postMessage(m: unknown): void {
		this.posted.push(m);
	}
	addEventListener(_type: string, cb: (e: MessageEvent) => void): void {
		this.listeners.push(cb);
	}
	emit(data: unknown): void {
		for (const cb of this.listeners) cb({ data } as MessageEvent);
	}
	terminate(): void {}
}

let bindViewer: typeof import('./frames').bindViewer;
let dropRate: typeof import('./frames').dropRate;
let latestFrame: typeof import('./frames').latestFrame;
let latestHead: typeof import('./frames').latestHead;

/** Let the reconcile microtask run. */
const settle = (): Promise<void> => Promise.resolve();

let seq = 0;
/** Bind a viewer that constrains nothing, with a token of its own. */
const bind = (
	node: string,
	slot: string,
	cb: (f: DataFrame) => void = () => {}
): (() => void) => bindViewer(node, slot, `v${++seq}`, null, cb);

const line = (max: number) => ({
	dtype: 'array' as const,
	ndim: [['le', 3]] as [import('$lib/viewers/module').DimCmp, number][],
	reduce: [{ dim: -1, max }]
});

const opsOf = (w: MockWorker, op: string): unknown[] =>
	w.posted.filter((m) => (m as { op: string }).op === op);

beforeEach(async () => {
	vi.resetModules();
	vi.useFakeTimers();
	MockWorker.instances = [];
	vi.stubGlobal('Worker', MockWorker as unknown as typeof Worker);
	vi.stubGlobal('URL', URL);
	seq = 0;
	({ bindViewer, dropRate, latestFrame, latestHead } = await import('./frames'));
});

afterEach(() => {
	vi.useRealTimers();
	vi.unstubAllGlobals();
});

describe('one paint loop', () => {
	it('delivers every stream of a batch at once, and takes the paint count from the worker', async () => {
		// The worker's flush is the page's one paint loop: a batch is one of its flushes. Nothing
		// here paints on its own, so the HUD's "fps" is the worker's count and never a sum of two.
		const { paints } = await import('./frames');
		const delivered = vi.spyOn(paints, 'add');
		const gotA: DataFrame[] = [];
		const gotB: DataFrame[] = [];
		const offA = bind('osc-a', 'out', (f) => gotA.push(f));
		const offB = bind('osc-b', 'out', (f) => gotB.push(f));
		await settle(); // the reconcile opens the streams
		const w = MockWorker.instances[0];
		w.emit({
			batch: [
				{ node: 'osc-a', slot: 'out', frame: { shape: [1] } as unknown as DataFrame },
				{ node: 'osc-b', slot: 'out', frame: { shape: [2] } as unknown as DataFrame }
			]
		});
		expect(gotA.length, 'stream A delivered in the batch').toBe(1);
		expect(gotB.length, 'stream B delivered in the same batch').toBe(1);
		expect(delivered, 'a batch is not counted here').not.toHaveBeenCalled();
		w.emit({ stats: { paints: 2, streams: [] } });
		expect(delivered).toHaveBeenCalledWith(2);
		offA();
		offB();
	});
});

describe('a joining viewer', () => {
	it('replays the slot’s current frame to a late-joining consumer, immediately and once', async () => {
		// The bridge only sends when something changed (an emit, a joiner IT can see, a spec
		// change) — but a consumer joining a stream that is already open in THIS page is
		// invisible to it: no worker traffic happens at all. The frame it needs is already
		// cached on the slot, so the join replays it; without that, a metadata panel joining
		// a slot viewer's stream stares at its empty state until the producer's next emit —
		// ~10 s for a sparse producer, forever for a stopped one.
		const gotA: DataFrame[] = [];
		const offA = bind('osc', 'out', (f) => gotA.push(f));
		await settle();
		const frame1 = { shape: [1] } as unknown as DataFrame;
		MockWorker.instances[0].emit({ batch: [{ node: 'osc', slot: 'out', frame: frame1 }] });
		expect(gotA, 'the first consumer painted the frame').toEqual([frame1]);

		// The late joiner: no new worker frame, no timers — the cached frame arrives at once.
		const gotB: DataFrame[] = [];
		const offB = bind('osc', 'out', (f) => gotB.push(f));
		expect(gotB, 'a late joiner is served the cached current frame').toEqual([frame1]);
		expect(gotA, 'the replay reaches only the joiner, not the settled consumers').toEqual([
			frame1
		]);

		// And it is a replay, not a re-delivery loop: nothing further arrives unprompted.
		await vi.advanceTimersByTimeAsync(200);
		expect(gotB).toEqual([frame1]);
		offA();
		offB();
	});

	it('a joiner of a slot with no frame yet is not called', () => {
		const got: DataFrame[] = [];
		const off = bind('osc', 'out', (f) => got.push(f));
		expect(got).toEqual([]);
		off();
	});
});

describe('a held frame’s stamps', () => {
	it('land on the latest frame as a new object, without a delivery', async () => {
		// The reducer sends a frame that says what the last one said as its stamps alone. The
		// metadata panel polls `latestFrame`, so the stamps must show there; the viewers drew
		// nothing new, so no callback runs.
		const got: DataFrame[] = [];
		const off = bind('osc', 'out', (f) => got.push(f));
		await settle();
		const w = MockWorker.instances[0];
		const frame = { dtype: 'ARRAY', data: { dtype: '<f4', shape: [1], values: [3] }, meta: { time: 1, index: 1 } };
		w.emit({ batch: [{ node: 'osc', slot: 'out', frame }] });
		expect(got.length).toBe(1);
		const before = latestFrame('osc', 'out');
		w.emit({ batch: [{ node: 'osc', slot: 'out', stamps: { time: 2, index: 2 } }] });
		const after = latestFrame('osc', 'out');
		expect(after).not.toBe(before);
		expect(after?.meta).toEqual({ time: 2, index: 2 });
		expect(after?.data, 'the body is the held one').toBe(frame.data);
		expect(got.length, 'stamps alone deliver nothing').toBe(1);
		off();
	});
});

describe('per-stream drop accounting', () => {
	it('takes a stream’s drops from the worker, which coalesces there, and a drawn stream’s head', async () => {
		// The worker coalesces every stream, drawn there or read here, so a drop is counted once,
		// by the stream that overwrote a frame — never summed app-wide beside the paint count.
		// A viewer that draws in the worker asks for no frame here: the stream opens without
		// frames, and the head is all this thread holds.
		const off = bindViewer('osc', 'out', 'd', [line(256)], null);
		await settle();
		const w = MockWorker.instances[0];
		expect(opsOf(w, 'sub')).toEqual([{ op: 'sub', node: 'osc', slot: 'out', frames: false }]);
		w.emit({ batch: [{ node: 'osc', slot: 'out', head: { dtype: 'ARRAY', meta: { index: 7 }, array: { dtype: '<f4', shape: [8] } } }] });
		expect(latestFrame('osc', 'out')).toBeNull();
		expect(latestHead('osc', 'out')?.meta.index).toBe(7);
		w.emit({ batch: [{ node: 'osc', slot: 'out', stamps: { index: 8 } }] });
		expect(latestHead('osc', 'out')?.meta.index, 'stamps land on the head').toBe(8);
		bind('quiet', 'out');
		await settle();
		w.emit({ stats: { paints: 3, streams: [['osc', 'out', 4, 2]] } });
		await vi.advanceTimersByTimeAsync(600);
		expect(dropRate('osc', 'out')).toBeGreaterThan(0);
		expect(dropRate('quiet', 'out'), 'the quiet stream is not charged for its neighbour').toBe(0);
		// A reader joining the drawn stream turns the frames on, and leaves the drop count with the worker.
		const offR = bind('osc', 'out');
		await settle();
		expect((opsOf(w, 'spec').at(-1) as { frames: boolean }).frames).toBe(true);
		off();
		offR();
	});

	it('reports null for a stream nobody is watching', async () => {
		// Absent is not zero: `0/s` for a stream that is not running asserts something false.
		expect(dropRate('osc-a', 'out')).toBeNull();
		const off = bind('osc-a', 'out');
		await settle();
		expect(dropRate('osc-a', 'out')).toBe(0);
		off();
		await settle();
		expect(dropRate('osc-a', 'out'), 'and null again once the last viewer leaves').toBeNull();
	});
});

/**
 * The registry is the ONLY thing that decides what the backend is asked for, and it is read once
 * the tick has settled rather than per mutation. That is what makes a viewer's comings and goings
 * invisible to the backend unless they actually change what it should serve.
 */
describe('what the registry tells the backend', () => {
	it('drops a viewer removed before its stream opens', async () => {
		const off = bind('osc', 'out');
		off();
		await settle();
		expect(MockWorker.instances).toEqual([]);
		expect(dropRate('osc', 'out')).toBeNull();
	});

	it('keeps the current binding when an old binding releases the same callback', async () => {
		for (const cb of [null, () => {}]) {
			const off = bindViewer('osc', 'out', 'a', [line(150)], cb);
			const current = bindViewer('osc', 'out', 'a', [line(320)], cb);
			off();
			await settle();
			const w = MockWorker.instances[0];
			expect((opsOf(w, 'spec').at(-1) as { specs: unknown[] }).specs).toEqual([line(320)]);
			expect(dropRate('osc', 'out')).toBe(0);
			current();
			await settle();
			expect(dropRate('osc', 'out')).toBeNull();
		}
	});

	it('opens once for a slot, whatever the viewer count, and closes when the last one goes', async () => {
		const offA = bind('osc', 'out');
		const offB = bind('osc', 'out');
		await settle();
		const w = MockWorker.instances[0];
		expect(opsOf(w, 'sub'), 'two viewers, one stream').toEqual([
			{ op: 'sub', node: 'osc', slot: 'out', frames: true }
		]);

		offA();
		await settle();
		expect(opsOf(w, 'unsub'), 'one viewer left, and it is still watched').toEqual([]);
		offB();
		await settle();
		expect(opsOf(w, 'unsub')).toEqual([{ op: 'unsub', node: 'osc', slot: 'out' }]);
	});

	it('says NOTHING when a viewer leaves a slot another viewer is still on', async () => {
		// The failure this replaces: the node's own viewer closing tore down the stream a panel
		// bound to the same slot was drawing from. Nothing about what the backend should serve
		// changed, so nothing should reach it — not an unsub, not a re-sub, not a spec.
		const offA = bindViewer('osc', 'out', 'a', [line(256)], () => {});
		bindViewer('osc', 'out', 'b', [line(256)], () => {});
		await settle();
		const w = MockWorker.instances[0];
		const before = w.posted.length;

		offA();
		await settle();
		expect(w.posted.length, `the backend was told: ${JSON.stringify(w.posted.slice(before))}`).toBe(
			before
		);
	});

	it('says nothing when a viewer detaches and re-attaches inside one tick', async () => {
		// What a re-render does. The registry ends the tick exactly as it started, so the reconcile
		// finds nothing to say — no socket churn, and no cached frame thrown away.
		const off = bindViewer('osc', 'out', 'a', [line(256)], () => {});
		await settle();
		const w = MockWorker.instances[0];
		const before = w.posted.length;

		off();
		bindViewer('osc', 'out', 'a', [line(256)], () => {});
		await settle();
		expect(w.posted.length, 'a detach and re-attach is not an event').toBe(before);
	});

	it('sends the specs only when the list actually changes', async () => {
		bindViewer('osc', 'out', 'a', [line(150)], () => {});
		await settle();
		const w = MockWorker.instances[0];
		expect(opsOf(w, 'spec')).toEqual([
			{ op: 'spec', node: 'osc', slot: 'out', specs: [line(150)], frames: true }
		]);

		// Re-binding the same viewer with the same need — a re-render — says nothing.
		bindViewer('osc', 'out', 'a', [line(150)], () => {});
		await settle();
		expect(opsOf(w, 'spec'), 'an unchanged need is not renegotiated').toHaveLength(1);

		// A resize is a real change, and it is sent.
		bindViewer('osc', 'out', 'a', [line(320)], () => {});
		await settle();
		expect(opsOf(w, 'spec')).toHaveLength(2);
		expect((opsOf(w, 'spec').at(-1) as { specs: unknown[] }).specs).toEqual([line(320)]);
	});

	it('collects every viewer’s need, and lets a reader contribute none', async () => {
		// Sent verbatim as a LIST: the bridge folds them, because only it has the real frame to
		// drop the ones a shape rules out. A null-spec viewer (the metadata panel) reads the same
		// stream without narrowing it to a budget it never asked for.
		bindViewer('osc', 'out', 'wide', [line(2000)], () => {});
		bindViewer('osc', 'out', 'narrow', [line(150)], () => {});
		bindViewer('osc', 'out', 'reader', null, () => {});
		await settle();
		const w = MockWorker.instances[0];
		expect((opsOf(w, 'spec').at(-1) as { specs: unknown[] }).specs).toEqual([
			line(2000),
			line(150)
		]);
	});
});
