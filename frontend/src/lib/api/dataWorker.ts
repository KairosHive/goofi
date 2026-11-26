/** Data-plane Web Worker: one WebSocket per (node, slot), each frame decoded on arrival and drawn
 * here; a frame goes to the main thread only where a reader there asked for it. */
import { decodeData, decodeStamps, isArrayFrame, type DataFrame } from '$lib/codec/decode';
import { createRenderer, type Renderer, type Surface } from '$lib/plotluck';
import type { DrawBox, Drawing, DrawnState } from '$lib/viewers/drawing';
import type { ProbeBox } from '$lib/viewers/hover';
import { isRenderable, makeDrawing, type ViewerKind } from '$lib/viewers/registry';
import type { SettingsMap } from '$lib/viewers/module';
import { summaryOf } from '$lib/viewers/viewMeta';
import { headOf, streamKey, type Drawn, type StreamNews, type ToMain, type ToWorker } from './dataProtocol';
import { wsUrl } from './wsUrl';

interface SlotState {
	node: string;
	slot: string;
	ws: WebSocket | null;
	url: string;
	/** Whether a 'sub' has opened the socket; a drawing may attach before the registry settles. */
	opened: boolean;
	closed: boolean;
	reconnectMs: number;
	/** The ViewSpecs every viewer of this slot has contributed; empty means full resolution. */
	specs: unknown[];
	/** Whether the main thread reads this stream's frames. */
	frames: boolean;
	/** Whether the main thread asked for this stream's heads; until then a frame's summary is not built. */
	heads: boolean;
	/** The drawings this stream feeds. */
	drawings: Set<DrawingState>;
	/** The last decoded frame, replayed to a drawing that attaches. */
	latest: DataFrame | null;
	/** The newest frame off the wire, decoded at the next flush, and the stamps that came after it. */
	pending: ArrayBuffer | null;
	pendingStamps: Record<string, unknown> | null;
	/** What the main thread is owed at the next flush, latest-wins. */
	news: Omit<StreamNews, 'node' | 'slot'> | null;
	/** Since the last stats report: what the wire delivered, and what arrived over an unflushed frame. */
	arrivals: number;
	drops: number;
	/** A frame arrived and no flush has run since. */
	unflushed: boolean;
}

interface DrawingState {
	id: number;
	drawing: Drawing;
	kind: ViewerKind;
	settings: SettingsMap;
	/** Unmeasured until the main thread says the body's size; nothing draws before. */
	box: DrawBox | null;
	slot: SlotState | null;
	pointer: { x: number; y: number; box: ProbeBox } | null;
	/** What the main thread was last told, so a frame that changes nothing there says nothing. */
	shown: string;
	hover: string;
}

const slots = new Map<string, SlotState>();
const surfaces = new Map<number, Surface>();
/** The one WebGL context, made with the first surface; a worker is allowed few. */
let renderer: Renderer | null = null;
const drawings = new Map<number, DrawingState>();
/** The page's display rate, declared with every stream's specs; unset until it is measured. */
let fps: number | undefined;
/** Whether the page is hidden: a hidden page holds no socket, so the manager serves it nothing. */
let hidden = false;

const send = (m: ToMain): void => (self as unknown as Worker).postMessage(m);

function sendSpecs(st: SlotState): void {
	if (!st.ws || st.ws.readyState !== WebSocket.OPEN) return;
	try {
		st.ws.send(JSON.stringify({ op: 'view', specs: st.specs, fps }));
	} catch {
		// Closed mid-send; the next (re)connect re-sends from st.specs.
	}
}

function openWs(st: SlotState): void {
	if (st.closed || hidden || st.ws) return;
	const ws = new WebSocket(st.url);
	ws.binaryType = 'arraybuffer';
	st.ws = ws;
	ws.addEventListener('open', () => {
		st.reconnectMs = 250;
		sendSpecs(st); // no server resume
	});
	ws.addEventListener('message', (e) => {
		if (e.data instanceof ArrayBuffer) arrive(st, e.data);
	});
	ws.addEventListener('close', (e) => {
		if (st.ws === ws) st.ws = null;
		if (st.closed || hidden) return;
		// Terminal app close codes (4000+) can never resolve — don't reconnect.
		if (e.code >= 4000) {
			st.closed = true;
			return;
		}
		const delay = st.reconnectMs;
		st.reconnectMs = Math.min(st.reconnectMs * 2, 5000);
		setTimeout(() => openWs(st), delay);
	});
}


// ── paints and stats ─────────────────────────────────────────────────────────────────────────
// THE page's one paint loop: a flush renders every stale drawing from its stream's latest frame
// and hands the main thread what its readers are owed in ONE message.
const schedule =
	typeof requestAnimationFrame === 'function'
		? requestAnimationFrame
		: (fn: () => void): number => setTimeout(fn, 16) as unknown as number;
const stale = new Set<DrawingState>();
/** The states a flush's drawings reached, sent with its batch; null outside a flush. */
let shows: Drawn[] | null = null;
let painting = false;
let paints = 0;
/** How many flushes in a row may find nothing before the loop lets go. */
const QUIET = 30;
let quiet = 0;
/** An animation frame asked for after an idle gap is answered at once, which splits one tick's
 * streams into two paints: the loop stays armed between ticks, and a re-arm paints a frame late. */
const arm =
	typeof requestAnimationFrame === 'function'
		? (fn: () => void): number => requestAnimationFrame(() => requestAnimationFrame(fn))
		: schedule;
function requestFlush(): void {
	if (painting) return;
	painting = true;
	arm(paint);
}
function paint(): void {
	quiet = flush() ? 0 : quiet + 1;
	if ((slots.size === 0 && stale.size === 0) || quiet > QUIET) {
		painting = false;
		quiet = 0;
		return;
	}
	schedule(paint);
}
function invalidate(d: DrawingState): void {
	stale.add(d);
	requestFlush();
}
/** One flush; whether it found a frame, a drawing or news to act on. */
function flush(): boolean {
	let painted = false;
	let worked = stale.size > 0;
	const drawn: Drawn[] = (shows = []);
	for (const st of slots.values()) {
		if (!st.pending) continue;
		worked = true;
		take(st);
	}
	for (const d of stale) {
		if (!d.slot?.latest) continue;
		// One drawing that cannot draw this frame must not withhold the batch from everyone.
		try {
			render(d, d.slot.latest);
		} catch (e) {
			console.error('viewer draw failed', e);
		}
		painted = true;
	}
	stale.clear();
	const batch: StreamNews[] = [];
	for (const st of slots.values()) {
		st.unflushed = false;
		if (!st.news) continue;
		if (st.news.frame) painted = true;
		batch.push({ node: st.node, slot: st.slot, ...st.news });
		st.news = null;
	}
	shows = null;
	// Copied, never transferred: the slot keeps the frame to replay to a drawing that attaches.
	if (drawn.length > 0) send({ batch, drawn });
	else if (batch.length > 0) send({ batch });
	if (painted) paints++;
	return worked || batch.length > 0;
}
let statsTimer: ReturnType<typeof setInterval> | null = null;
function report(): void {
	const streams: [string, string, number, number][] = [];
	for (const st of slots.values()) {
		if (st.arrivals === 0 && st.drops === 0) continue;
		streams.push([st.node, st.slot, st.arrivals, st.drops]);
		st.arrivals = 0;
		st.drops = 0;
	}
	if (paints === 0 && streams.length === 0) {
		// No stream left to count: the next 'sub' starts the timer again.
		if (slots.size === 0 && statsTimer !== null) {
			clearInterval(statsTimer);
			statsTimer = null;
		}
		return;
	}
	send({ stats: { paints, streams } });
	paints = 0;
}

// ── drawings ─────────────────────────────────────────────────────────────────────────────────
function render(d: DrawingState, f: DataFrame): void {
	if (!d.box) return;
	let state: DrawnState;
	if (!isArrayFrame(f) || !isRenderable(d.kind, f.data, d.settings)) {
		// A frame this kind cannot draw takes the fallback text; the trace before it must not stay under it.
		d.drawing.clear();
		state = { ...stateOf(d, true), fallback: isArrayFrame(f) ? summaryOf(f.data, f.meta) : null };
	} else {
		d.drawing.push(f, d.settings, d.box);
		state = stateOf(d, true);
	}
	show(d, state);
	probe(d);
}

function stateOf(d: DrawingState, has: boolean): DrawnState {
	const w = d.drawing;
	return { has, fallback: null, labels: w.labels, texts: w.texts, message: w.message, drag: w.drag !== null };
}

function show(d: DrawingState, state: DrawnState): void {
	const sig = JSON.stringify(state);
	if (sig === d.shown) return;
	d.shown = sig;
	const drawn = { id: d.id, ...state };
	if (shows) shows.push(drawn);
	else send({ batch: [], drawn: [drawn] });
}

/** Ask the drawing's probe about the pointer; the readout follows the pointer and the frame alike. */
function probe(d: DrawingState): void {
	const p = d.pointer;
	const hover = p && d.drawing.probe ? d.drawing.probe(p.x, p.y, p.box) : null;
	const sig = JSON.stringify(hover);
	if (sig === d.hover) return;
	d.hover = sig;
	send({ hover: { id: d.id, hover } });
}

function clearDrawing(d: DrawingState): void {
	d.drawing.clear();
	show(d, stateOf(d, false));
	probe(d);
}

function detach(d: DrawingState): void {
	d.slot?.drawings.delete(d);
	d.slot = null;
	stale.delete(d);
	clearDrawing(d);
}

// ── messages ─────────────────────────────────────────────────────────────────────────────────
/** The slot record for a stream, made on first mention: by 'sub', or by a drawing's 'attach'
 * that lands before the registry has settled its demand. */
function ensureSlot(node: string, slot: string): SlotState {
	const k = streamKey(node, slot);
	let st = slots.get(k);
	if (!st) {
		st = {
			node,
			slot,
			ws: null,
			url: wsUrl(['data', node, slot], self.location),
			opened: false,
			closed: false,
			reconnectMs: 250,
			specs: [],
			frames: false,
			heads: false,
			drawings: new Set(),
			latest: null,
			pending: null,
			pendingStamps: null,
			news: null,
			arrivals: 0,
			drops: 0,
			unflushed: false
		};
		slots.set(k, st);
	}
	return st;
}

self.addEventListener('message', (e: MessageEvent) => {
	const m = e.data as ToWorker;
	switch (m.op) {
		case 'rate':
			fps = m.fps;
			for (const st of slots.values()) sendSpecs(st);
			break;
		case 'hidden':
			hidden = m.hidden;
			for (const st of slots.values()) {
				if (hidden) st.ws?.close();
				else if (st.opened) openWs(st);
			}
			break;
		case 'sub': {
			const st = ensureSlot(m.node, m.slot);
			if (st.opened) break;
			st.opened = true;
			st.frames = m.frames;
			statsTimer ??= setInterval(report, 250);
			openWs(st);
			requestFlush();
			break;
		}
		case 'head': {
			// The ask can precede the settled tick's 'sub'; the slot it makes is the one 'sub' opens.
			const st = ensureSlot(m.node, m.slot);
			if (st.heads) break;
			st.heads = true;
			if (st.latest && !st.frames) st.news = { head: headOf(st.latest) };
			requestFlush();
			break;
		}
		case 'spec': {
			// A 'sub' always precedes a 'spec', so a spec for an absent slot is a post-unsub straggler.
			const st = slots.get(streamKey(m.node, m.slot));
			if (!st) break;
			st.specs = m.specs;
			st.frames = m.frames;
			sendSpecs(st);
			break;
		}
		case 'unsub': {
			const k = streamKey(m.node, m.slot);
			const st = slots.get(k);
			if (!st) break;
			st.closed = true;
			st.ws?.close();
			for (const d of st.drawings) d.slot = null;
			slots.delete(k);
			break;
		}
		case 'surface':
			try {
				renderer ??= createRenderer();
				surfaces.set(m.id, renderer.surface(m.canvas));
			} catch (err) {
				console.warn(err);
				send({ surface: { id: m.id, ok: false } });
			}
			break;
		case 'view':
			surfaces.get(m.id)?.setView(m.view);
			break;
		case 'dispose':
			surfaces.get(m.id)?.dispose();
			surfaces.delete(m.id);
			break;
		case 'drawing': {
			const surface = surfaces.get(m.surface);
			if (!surface) break;
			drawings.set(m.id, {
				id: m.id,
				drawing: makeDrawing(m.kind, surface, m.variant),
				kind: m.kind,
				settings: {},
				box: null,
				slot: null,
				pointer: null,
				shown: '',
				hover: ''
			});
			break;
		}
		case 'drop': {
			const d = drawings.get(m.id);
			if (!d) break;
			d.slot?.drawings.delete(d);
			stale.delete(d);
			d.drawing.remove();
			drawings.delete(m.id);
			break;
		}
		case 'place':
			drawings.get(m.id)?.drawing.place(m.x, m.y, m.w, m.h, m.z);
			break;
		case 'background':
			drawings.get(m.id)?.drawing.setBackground(m.hex);
			break;
		case 'settings': {
			const d = drawings.get(m.id);
			if (!d) break;
			d.settings = m.settings;
			d.box = m.box;
			invalidate(d);
			break;
		}
		case 'attach': {
			const d = drawings.get(m.id);
			if (!d) break;
			const st = ensureSlot(m.node, m.slot);
			if (d.slot !== st) detach(d);
			d.slot = st;
			st.drawings.add(d);
			invalidate(d);
			break;
		}
		case 'detach': {
			const d = drawings.get(m.id);
			if (d) detach(d);
			break;
		}
		case 'pointer': {
			const d = drawings.get(m.id);
			if (!d) break;
			d.pointer = m.at;
			probe(d);
			break;
		}
		case 'drag': {
			const d = drawings.get(m.id);
			if (!d?.drawing.drag) break;
			d.drawing.drag(m.dx, m.dy, m.box);
			show(d, stateOf(d, true));
			probe(d);
			break;
		}
	}
});

/** Keep the newest frame off the wire for the next flush, which decodes it: a frame another
 * overwrites first is never decoded. Stamps land on the frame they follow. */
function arrive(st: SlotState, raw: ArrayBuffer): void {
	let stamps: Record<string, unknown> | null;
	try {
		stamps = decodeStamps(raw);
	} catch {
		return; // a corrupt frame shouldn't kill the slot
	}
	if (stamps) {
		if (st.pending) st.pendingStamps = { ...st.pendingStamps, ...stamps };
		else restamp(st, stamps);
		requestFlush();
		return;
	}
	st.arrivals++;
	// A frame over one no flush has shown yet is a drop, whoever would have drawn it.
	if (st.unflushed) st.drops++;
	st.unflushed = true;
	st.pending = raw;
	st.pendingStamps = null;
	requestFlush();
}

/** Decode the pending frame and mark what it changes: the drawings it feeds, and the news the
 * main thread is owed (the frame where a reader asked, else its head). */
function take(st: SlotState): void {
	const raw = st.pending;
	const stamps = st.pendingStamps;
	st.pending = null;
	st.pendingStamps = null;
	if (!raw) return;
	let frame: DataFrame;
	try {
		frame = decodeData(raw);
	} catch {
		return; // a corrupt frame shouldn't kill the slot
	}
	st.latest = frame;
	for (const d of st.drawings) stale.add(d);
	if (st.frames) st.news = { frame };
	else if (st.heads) st.news = { head: headOf(frame) };
	if (stamps) restamp(st, stamps);
}

/** A held frame's fresh stamps, on the frame and on whatever news the main thread is owed. */
function restamp(st: SlotState, stamps: Record<string, unknown>): void {
	if (st.latest) st.latest = { ...st.latest, meta: { ...st.latest.meta, ...stamps } };
	const n = st.news;
	if (n?.frame) n.frame = { ...n.frame, meta: { ...n.frame.meta, ...stamps } };
	else if (n?.head) n.head = { ...n.head, meta: { ...n.head.meta, ...stamps } };
	else if (st.frames || st.heads) st.news = { stamps: { ...n?.stamps, ...stamps } };
}
