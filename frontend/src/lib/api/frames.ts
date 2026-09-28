/** The viewer registry: what each stream is asked for, and the frames this thread reads. A
 * drawing consumes its frames in the worker; a reader here gets them on ONE rAF flush per tick,
 * most-starved slot first, with a per-frame time budget. */
import { closeStream, declareRate, listen, openStream, sendSpecs } from './data';
import { headOf, type FrameHead } from './dataProtocol';
import { perfStats } from './perfStats.svelte';
import { RateMeter } from './rateMeter';
import type { DataFrame } from '$lib/codec/decode';
import type { ViewSpec } from '$lib/viewers/capacity';
import { streamKey } from './streamKey';
import { flushSync } from 'svelte';

type FrameCallback = (frame: DataFrame) => void;

/** One viewer bound to a stream. Null `specs` contribute nothing to the reduction; a stream
 * every viewer declares null for is served one texel, never the full frame. A null `cb` is a
 * drawing the worker feeds, which asks for no frame here. */
interface BoundViewer {
	cb: FrameCallback | null;
	specs: ViewSpec[] | null;
}

interface Slot {
	/** Every viewer bound to this stream, by its own stable token. THE registry: nothing else
	 *  counts viewers, and nothing else decides what the backend is asked for. */
	viewers: Map<string, BoundViewer>;
	pending: DataFrame | null;
	current: DataFrame | null;
	/** What the last frame said of itself, for a stream no reader here holds. */
	head: FrameHead | null;
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
			if (!cb) continue;
			try {
				cb(frame);
			} catch (err) {
				console.error('frame consumer crashed', err);
			}
		}
		// A component viewer's draws run here, inside the budget; the worker's plots draw in their own frame.
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
	const { specs, frames } = JSON.parse(want) as Demand;
	if (have === null) openStream(node, slot, frames);
	sendSpecs(node, slot, specs, frames);
	synced.set(k, want);
}

interface Demand {
	specs: ViewSpec[];
	/** Whether any viewer reads the frames on this thread. */
	frames: boolean;
}

/** What this page needs of a stream: every bound viewer's constraint, DISTINCT ones only —
 * the bridge folds richest-per-dim, so a repeat would renegotiate nothing — and whether the
 * frames are wanted here. */
function demand(s: Slot): Demand {
	const seen = new Set<string>();
	const specs: ViewSpec[] = [];
	let frames = false;
	for (const v of s.viewers.values()) {
		if (v.cb) frames = true;
		for (const spec of v.specs ?? []) {
			const sig = JSON.stringify(spec);
			if (seen.has(sig)) continue;
			seen.add(sig);
			specs.push(spec);
		}
	}
	return { specs, frames };
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
			head: null,
			lastFlush: 0,
			drops: new RateMeter(nowMs()),
			arrivals: new RateMeter(nowMs())
		};
		slots.set(k, s);
	}
	return s;
}

/** Bind a viewer to a (node, slot) stream under `token`, so several viewers of one slot collect
 * rather than evict. Re-binding with changed `specs` reports a resize or a kind switch. A
 * viewer with no `cb` draws in the worker and only states its demand here. */
export function bindViewer(
	node: string,
	slot: string,
	token: string,
	specs: ViewSpec[] | null,
	cb: FrameCallback | null
): () => void {
	measureDisplayRate();
	const k = streamKey(node, slot);
	const s = ensureSlot(k);
	s.viewers.set(token, { cb, specs });
	scheduleReconcile(node, slot, k);
	// An open stream sends nothing new for a joiner, so replay the current frame to it alone —
	// re-marking the slot dirty would repaint every settled viewer.
	if (cb && s.current) {
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

/** Whether the worker draws this stream for a viewer here. */
function drawn(s: Slot): boolean {
	for (const v of s.viewers.values()) if (!v.cb) return true;
	return false;
}

listen((m) => {
	if ('frame' in m) {
		// Everything the worker decodes for this thread lands here, latest-wins, painted on the next flush.
		const s = slots.get(streamKey(m.node, m.slot));
		if (!s) return;
		// A pending frame overwritten before it painted is a drop, charged to THIS stream; where
		// the worker draws the stream too, its count stands for both.
		if (s.pending !== null && !drawn(s)) s.drops.dropped();
		s.pending = m.frame;
		s.head = null;
		dirty.add(s);
		requestFlush();
	} else if ('head' in m) {
		const s = slots.get(streamKey(m.node, m.slot));
		if (s) s.head = m.head;
	} else if ('stamps' in m) {
		// A held frame's fresh stamps land on the pending frame, else the painted one, as a new
		// object a poll sees; nothing is marked dirty, because nothing on screen changed.
		const s = slots.get(streamKey(m.node, m.slot));
		if (!s) return;
		const restamp = (f: DataFrame): DataFrame => ({ ...f, meta: { ...f.meta, ...m.stamps } });
		if (s.pending) s.pending = restamp(s.pending);
		if (s.current) s.current = restamp(s.current);
		else if (s.head) s.head = { ...s.head, meta: { ...s.head.meta, ...m.stamps } };
	} else if ('stats' in m) {
		// The worker's paints and, per stream, what the wire delivered and what its plots
		// coalesced: a drop belongs to the stream whose frame was overwritten before a draw.
		if (m.stats.paints > 0) perfStats().delivered(m.stats.paints);
		for (const [node, slot, arrivals, drops] of m.stats.streams) {
			const s = slots.get(streamKey(node, slot));
			if (!s) continue;
			s.arrivals.delivered(arrivals);
			s.drops.dropped(drops);
		}
	}
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

/** The latest frame for a (node, slot), or null when no reader on this thread holds one. */
export function latestFrame(node: string, slot: string): DataFrame | null {
	return slots.get(streamKey(node, slot))?.current ?? null;
}

/** What the latest frame said of itself: where a reader on this thread takes the frames, the
 * one it was given, never one still pending its flush; else what the worker told of a frame it
 * kept for a drawing. Null when nothing is subscribed or nothing has been handed over yet. */
export function latestHead(node: string, slot: string): FrameHead | null {
	const s = slots.get(streamKey(node, slot));
	if (!s) return null;
	if (s.current) return headOf(s.current);
	return demand(s).frames ? null : s.head;
}
