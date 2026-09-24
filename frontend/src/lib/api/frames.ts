/** The viewer registry and display-rate frame delivery: ONE rAF flush per tick, most-starved
 * slot first, with a per-frame time budget. */
import { closeStream, declareRate, openStream, sendSpecs, setFrameSink, setStampsSink } from './data';
import { perfStats } from './perfStats.svelte';
import { RateMeter } from './rateMeter';
import type { DataFrame } from '$lib/codec/decode';
import type { ViewSpec } from '$lib/viewers/capacity';
import { streamKey } from './streamKey';
import { flushSync } from 'svelte';

type FrameCallback = (frame: DataFrame) => void;

/** One viewer bound to a stream. Null `specs` contribute nothing to the reduction; a stream
 * every viewer declares null for is served one texel, never the full frame. */
interface BoundViewer {
	cb: FrameCallback;
	specs: ViewSpec[] | null;
}

interface Slot {
	/** Every viewer bound to this stream, by its own stable token. THE registry: nothing else
	 *  counts viewers, and nothing else decides what the backend is asked for. */
	viewers: Map<string, BoundViewer>;
	pending: DataFrame | null;
	current: DataFrame | null;
	/** When this slot last delivered, for most-starved-first fairness. */
	lastFlush: number;
	/** This stream's own coalescing rate — per slot, because a drop belongs to the stream that
	 * overwrote a frame. */
	drops: RateMeter;
	/** What the WIRE delivered, which the paint rate cannot show: the cap the manager serves at
	 * is invisible in a paint count, since a paint is capped either way. */
	arrivals: RateMeter;
}

const slots = new Map<string, Slot>();
const dirty = new Set<Slot>();
const FRAME_BUDGET_MS = 8;

const nowMs =
	typeof performance !== 'undefined' && typeof performance.now === 'function'
		? (): number => performance.now()
		: (): number => Date.now();

const scheduleFlush =
	typeof requestAnimationFrame === 'function'
		? (fn: () => void): number => requestAnimationFrame(fn)
		: (fn: () => void): number => setTimeout(fn, 16) as unknown as number;

let scheduled = false;
/** Paint what is pending on the next animation frame; the manager owns the rate it arrives at. */
function requestFlush(): void {
	if (scheduled) return;
	scheduled = true;
	scheduleFlush(() => {
		scheduled = false;
		flush();
	});
}

let measured = false;
/** Count animation frames for half a second and declare the display's rate to every socket, so
 * the manager serves no faster than this page can paint. */
function measureDisplayRate(): void {
	if (measured || typeof requestAnimationFrame !== 'function') return;
	measured = true;
	let start = -1;
	let frames = 0;
	const step = (t: number): void => {
		if (start < 0) start = t;
		else frames++;
		if (t - start >= 500) declareRate(Math.round((frames * 1000) / (t - start)));
		else requestAnimationFrame(step);
	};
	requestAnimationFrame(step);
}

function flush(): void {
	const start = nowMs();
	// Most-starved slot first, so the budget can never permanently defer a slot.
	const queue = [...dirty].sort((a, b) => a.lastFlush - b.lastFlush);
	let painted = 0;
	for (const s of queue) {
		// Always deliver at least one slot; after that, stop once over budget.
		if (painted > 0 && nowMs() - start > FRAME_BUDGET_MS) break;
		const frame = s.pending;
		dirty.delete(s);
		if (!frame) continue;
		s.pending = null;
		s.current = frame;
		s.lastFlush = start;
		painted++;
		for (const { cb } of [...s.viewers.values()]) {
			try {
				cb(frame);
			} catch (err) {
				console.error('frame consumer crashed', err);
			}
		}
		// A component viewer's draws run here, inside the budget; a surface plot draws in its own frame.
		flushSync();
	}
	// ONE paint per flush, not one per slot: that is the quantity the HUD names.
	if (painted > 0) perfStats().delivered();
	if (dirty.size > 0) requestFlush();
}


/** What the backend has been told to serve for a stream; absent means no stream is open. */
const synced = new Map<string, string>();
const reconciling = new Set<string>();

/** Bring the backend in line with the registry for one stream, read once the tick has SETTLED. */
function reconcile(node: string, slot: string, k: string): void {
	const s = slots.get(k);
	const want = s && s.viewers.size > 0 ? JSON.stringify(demand(s)) : null;
	const have = synced.get(k) ?? null;
	if (want === have) return;

	if (want === null) {
		closeStream(node, slot);
		synced.delete(k);
		if (s) dirty.delete(s);
		slots.delete(k);
		return;
	}
	if (have === null) openStream(node, slot);
	sendSpecs(node, slot, JSON.parse(want) as ViewSpec[]);
	synced.set(k, want);
}

/** What this page needs a stream reduced to: every bound viewer's constraint, DISTINCT ones only
 * — the bridge folds richest-per-dim, so a repeat would renegotiate nothing. */
function demand(s: Slot): ViewSpec[] {
	const seen = new Set<string>();
	const out: ViewSpec[] = [];
	for (const { specs } of s.viewers.values()) {
		for (const spec of specs ?? []) {
			const sig = JSON.stringify(spec);
			if (seen.has(sig)) continue;
			seen.add(sig);
			out.push(spec);
		}
	}
	return out;
}

function scheduleReconcile(node: string, slot: string, k: string): void {
	if (reconciling.has(k)) return;
	reconciling.add(k);
	queueMicrotask(() => {
		reconciling.delete(k);
		reconcile(node, slot, k);
	});
}

function ensureSlot(k: string): Slot {
	let s = slots.get(k);
	if (!s) {
		s = {
			viewers: new Map(),
			pending: null,
			current: null,
			lastFlush: 0,
			drops: new RateMeter(nowMs()),
			arrivals: new RateMeter(nowMs())
		};
		slots.set(k, s);
	}
	return s;
}

/** Bind a viewer to a (node, slot) stream under `token`, so several viewers of one slot collect
 * rather than evict. Re-binding with changed `specs` reports a resize or a kind switch. */
export function bindViewer(
	node: string,
	slot: string,
	token: string,
	specs: ViewSpec[] | null,
	cb: FrameCallback
): () => void {
	measureDisplayRate();
	const k = streamKey(node, slot);
	const s = ensureSlot(k);
	s.viewers.set(token, { cb, specs });
	scheduleReconcile(node, slot, k);
	// An open stream sends nothing new for a joiner, so replay the current frame to it alone —
	// re-marking the slot dirty would repaint every settled viewer.
	if (s.current) {
		try {
			cb(s.current);
		} catch (err) {
			console.error('frame consumer crashed', err);
		}
	}
	return () => {
		const cur = slots.get(k);
		if (cur?.viewers.get(token)?.cb !== cb) return; // already replaced by a later bind
		cur.viewers.delete(token);
		scheduleReconcile(node, slot, k);
	};
}

/** Everything the worker decodes lands here, latest-wins, and is painted on the next flush. */
setFrameSink((node, slot, frame) => {
	const s = slots.get(streamKey(node, slot));
	if (!s) return;
	s.arrivals.delivered();
	// A pending frame overwritten before it painted is a drop, charged to THIS stream.
	if (s.pending !== null) s.drops.dropped();
	s.pending = frame;
	dirty.add(s);
	requestFlush();
});

/** A held frame's fresh stamps land on the pending frame, else the painted one, as a new object
 * a poll sees; nothing is marked dirty, because nothing on screen changed. */
setStampsSink((node, slot, stamps) => {
	const s = slots.get(streamKey(node, slot));
	if (!s) return;
	const restamp = (f: DataFrame): DataFrame => ({ ...f, meta: { ...f.meta, ...stamps } });
	if (s.pending) s.pending = restamp(s.pending);
	else if (s.current) s.current = restamp(s.current);
});

/** Coalesced-frame rate for ONE stream. Null when nothing is subscribed: absent is not zero. */
export function dropRate(node: string, slot: string): number | null {
	const s = slots.get(streamKey(node, slot));
	if (!s) return null;
	s.drops.tick(nowMs());
	return s.drops.dps;
}

/** Frames a second arriving on ONE stream, before any coalescing. Null when nothing is
 * subscribed: absent is not zero. */
export function arrivalRate(node: string, slot: string): number | null {
	const s = slots.get(streamKey(node, slot));
	if (!s) return null;
	s.arrivals.tick(nowMs());
	return s.arrivals.fps;
}

/** The latest frame for a (node, slot), or null when nothing is subscribed to it. */
export function latestFrame(node: string, slot: string): DataFrame | null {
	return slots.get(streamKey(node, slot))?.current ?? null;
}
