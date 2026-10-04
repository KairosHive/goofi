/** The viewer registry: what each stream is asked for, and the frames this thread reads; a
 * drawing consumes its frames in the worker's paint loop, a reader here gets each flush's batch. */
import { listen, post } from './data';
import { headOf, streamKey, type FrameHead } from './dataProtocol';
import { RateMeter } from './rateMeter.svelte';
import type { DataFrame } from '$lib/codec/decode';
import type { ViewSpec } from '$lib/viewers/module';
import { flushSync } from 'svelte';

type FrameCallback = (frame: DataFrame) => void;

/** One viewer bound to a stream. Null `specs` contribute nothing to the reduction; a null `cb`
 * is a drawing the worker feeds, which asks for no frame here. */
interface BoundViewer {
	cb: FrameCallback | null;
	specs: ViewSpec[] | null;
}

interface Slot {
	/** Every viewer bound to this stream, by its own stable token. THE registry: nothing else
	 *  counts viewers, and nothing else decides what the backend is asked for. */
	viewers: Map<string, BoundViewer>;
	/** The demand sent to the worker; null until the stream is opened. */
	synced: string | null;
	reconciling: boolean;
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

/** The worker's paints a second, app-wide; drops are per stream (`dropRate`), never summed. */
export const paints = new RateMeter();

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
		if (t - start >= 500) post({ op: 'rate', fps: Math.round((frames * 1000) / (t - start)) });
		else requestAnimationFrame(step);
	};
	requestAnimationFrame(step);
}

/** Bring the backend in line with the registry for one stream, read once the tick has SETTLED. */
function reconcile(node: string, slot: string, k: string, s: Slot): void {
	if (s.viewers.size === 0) {
		if (s.synced !== null) post({ op: 'unsub', node, slot });
		slots.delete(k);
		return;
	}
	const { specs, frames } = demand(s);
	const want = JSON.stringify({ specs, frames });
	if (want === s.synced) return;
	if (s.synced === null) post({ op: 'sub', node, slot, frames });
	post({ op: 'spec', node, slot, specs, frames });
	s.synced = want;
}

interface Demand {
	specs: ViewSpec[];
	/** Whether any viewer reads the frames on this thread. */
	frames: boolean;
}

/** What this page needs of a stream: every bound viewer's DISTINCT constraint (the bridge folds
 * richest-per-dim, so a repeat renegotiates nothing) and whether the frames are wanted here. */
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

function scheduleReconcile(node: string, slot: string, k: string, s: Slot): void {
	if (s.reconciling) return;
	s.reconciling = true;
	queueMicrotask(() => {
		s.reconciling = false;
		reconcile(node, slot, k, s);
	});
}

function ensureSlot(k: string): Slot {
	let s = slots.get(k);
	if (!s) {
		s = {
			viewers: new Map(),
			synced: null,
			reconciling: false,
			current: null,
			head: null,
			drops: new RateMeter(),
			arrivals: new RateMeter()
		};
		slots.set(k, s);
	}
	return s;
}

function feed(cb: FrameCallback, frame: DataFrame): void {
	try {
		cb(frame);
	} catch (err) {
		console.error('frame consumer crashed', err);
	}
}

function deliver(s: Slot, frame: DataFrame): void {
	for (const { cb } of [...s.viewers.values()]) if (cb) feed(cb, frame);
}

/** Bind a viewer to a (node, slot) stream under `token`, so several viewers of one slot collect
 * rather than evict; a re-bind reports new `specs`, and a viewer with no `cb` draws in the worker. */
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
	const bound = { cb, specs };
	s.viewers.set(token, bound);
	scheduleReconcile(node, slot, k, s);
	// An open stream sends nothing new for a joiner, so replay the current frame to it alone.
	if (cb && s.current) feed(cb, s.current);
	return () => {
		const cur = slots.get(k);
		if (cur?.viewers.get(token) !== bound) return; // already replaced by a later bind
		cur.viewers.delete(token);
		scheduleReconcile(node, slot, k, cur);
	};
}

listen((m) => {
	if ('batch' in m) {
		// One flush of the worker, rendered in one turn: the page shows one batch, not one
		// stream after another.
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
				// Head news comes only while no reader here takes frames, so a held frame is stale.
				s.current = null;
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
		if (m.stats.paints > 0) paints.add(m.stats.paints);
		for (const [node, slot, arrivals, drops] of m.stats.streams) {
			const s = slots.get(streamKey(node, slot));
			if (!s) continue;
			s.arrivals.add(arrivals);
			s.drops.add(drops);
		}
	}
});

/** Coalesced-frame rate for ONE stream. Null when nothing is subscribed: absent is not zero. */
export function dropRate(node: string, slot: string): number | null {
	const s = slots.get(streamKey(node, slot));
	if (!s) return null;
	s.drops.tick();
	return s.drops.rate;
}

/** Frames a second arriving on ONE stream, before any coalescing. Null when nothing is
 * subscribed: absent is not zero. */
export function arrivalRate(node: string, slot: string): number | null {
	const s = slots.get(streamKey(node, slot));
	if (!s) return null;
	s.arrivals.tick();
	return s.arrivals.rate;
}

/** The latest frame for a (node, slot), or null when no reader on this thread holds one. */
export function latestFrame(node: string, slot: string): DataFrame | null {
	return slots.get(streamKey(node, slot))?.current ?? null;
}

/** What the latest frame said of itself: the frame a reader here was given, else the head the
 * worker told of one it kept for a drawing. Null before anything was handed over. */
export function latestHead(node: string, slot: string): FrameHead | null {
	const s = slots.get(streamKey(node, slot));
	if (!s) return null;
	if (s.current) return headOf(s.current);
	return demand(s).frames ? null : s.head;
}
