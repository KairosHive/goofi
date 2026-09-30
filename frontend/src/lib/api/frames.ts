/** The viewer registry: what each stream is asked for, and the frames this thread reads. The
 * worker owns the page's one paint loop: a drawing consumes its frames there, and a reader here
 * gets them in the batch each of its flushes sends. */
import { closeStream, declareRate, listen, openStream, sendSpecs } from './data';
import { headOf, type FrameHead } from './dataProtocol';
import { perfStats } from './perfStats.svelte';
import { RateMeter } from './rateMeter';
import type { DataFrame } from '$lib/codec/decode';
import type { ViewSpec } from '$lib/viewers/module';
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
	current: DataFrame | null;
	/** What the last frame said of itself, for a stream no reader here holds. */
	head: FrameHead | null;
	/** This stream's own coalescing rate, as the worker counts it — per slot, because a drop
	 * belongs to the stream that overwrote a frame. */
	drops: RateMeter;
	/** What the WIRE delivered, which the paint rate cannot show: the cap the manager serves at
	 * is invisible in a paint count, since a paint is capped either way. */
	arrivals: RateMeter;
}

const slots = new Map<string, Slot>();

const nowMs =
	typeof performance !== 'undefined' && typeof performance.now === 'function'
		? (): number => performance.now()
		: (): number => Date.now();

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
			current: null,
			head: null,
			drops: new RateMeter(nowMs()),
			arrivals: new RateMeter(nowMs())
		};
		slots.set(k, s);
	}
	return s;
}

function deliver(s: Slot, frame: DataFrame): void {
	for (const { cb } of [...s.viewers.values()]) {
		if (!cb) continue;
		try {
			cb(frame);
		} catch (err) {
			console.error('frame consumer crashed', err);
		}
	}
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
	// An open stream sends nothing new for a joiner, so replay the current frame to it alone.
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

listen((m) => {
	if ('batch' in m) {
		// One flush of the worker: every stream's news at once, and the readers' components
		// rendered in the same turn, so the page shows one batch, not one stream after another.
		let delivered = false;
		for (const n of m.batch) {
			const s = slots.get(streamKey(n.node, n.slot));
			if (!s) continue;
			if (n.frame) {
				s.current = n.frame;
				s.head = null;
				deliver(s, n.frame);
				delivered = true;
			} else if (n.head) {
				s.head = n.head;
			} else if (n.stamps) {
				// A held frame's fresh stamps land as a new object a poll sees; nothing on screen changed.
				const stamps = n.stamps;
				if (s.current) s.current = { ...s.current, meta: { ...s.current.meta, ...stamps } };
				else if (s.head) s.head = { ...s.head, meta: { ...s.head.meta, ...stamps } };
			}
		}
		if (delivered) flushSync();
	} else if ('stats' in m) {
		// The worker's paints and, per stream, what the wire delivered and what it coalesced.
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
 * one it was given; else what the worker told of a frame it kept for a drawing. Null when
 * nothing is subscribed or nothing has been handed over yet. */
export function latestHead(node: string, slot: string): FrameHead | null {
	const s = slots.get(streamKey(node, slot));
	if (!s) return null;
	if (s.current) return headOf(s.current);
	return demand(s).frames ? null : s.head;
}
