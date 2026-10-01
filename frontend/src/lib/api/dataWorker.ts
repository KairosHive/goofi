/** Data-plane Web Worker: one WebSocket per (node, slot), each frame decoded on arrival and
 * drawn here on the plot surfaces the main thread handed over; a frame goes to the main thread
 * only where a reader there asked for it. `frames.ts` owns the demand, `drawings.ts` the handles. */
import { decodeData, decodeStamps, isArrayFrame, type DataFrame } from '$lib/codec/decode';
import { createSurface, type Surface } from 'plotluck';
import type { DrawBox, Drawing, DrawnState } from '$lib/viewers/drawing';
import type { ProbeBox } from '$lib/viewers/hover';
import { isRenderable, makeDrawing, type ViewerKind } from '$lib/viewers/registry';
import type { SettingsMap } from '$lib/viewers/module';
import { summaryOf } from '$lib/viewers/viewMeta';
import { headOf, type StreamNews, type ToMain, type ToWorker } from './dataProtocol';
import { dataUrl } from './dataUrl';
import { streamKey } from './streamKey';

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
	/** The drawings this stream feeds. */
	drawings: Set<DrawingState>;
	/** The last decoded frame, replayed to a drawing that attaches. */
	latest: DataFrame | null;
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
const drawings = new Map<number, DrawingState>();
/** The page's display rate, declared with every stream's specs; unset until it is measured. */
let fps: number | undefined;

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
	if (st.closed) return;
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
		st.ws = null;
		if (st.closed) return;
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
let painting = false;
let paints = 0;
/** Keep the loop armed while a stream is open: a frame asked for after an idle gap is answered at
 * once, which split one tick's streams into two paints, so the loop never lets go between ticks. */
function requestFlush(): void {
	if (painting) return;
	painting = true;
	schedule(paint);
}
function paint(): void {
	flush();
	if (slots.size === 0 && stale.size === 0) {
		painting = false;
		return;
	}
	schedule(paint);
}
function invalidate(d: DrawingState): void {
	stale.add(d);
	requestFlush();
}
function flush(): void {
	let painted = false;
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
	// Copied, never transferred: the slot keeps the frame to replay to a drawing that attaches.
	if (batch.length > 0) send({ batch });
	if (painted) paints++;
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
	if (paints === 0 && streams.length === 0) return;
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
		state = {
			has: true,
			fallback: isArrayFrame(f) ? summaryOf(f.data, f.meta) : null,
			labels: [],
			texts: [],
			message: null,
			drag: false
		};
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
	send({ drawn: { id: d.id, ...state } });
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

function detach(d: DrawingState, keep: boolean): void {
	d.slot?.drawings.delete(d);
	d.slot = null;
	stale.delete(d);
	if (!keep) clearDrawing(d);
}

// ── messages ─────────────────────────────────────────────────────────────────────────────────
/** The slot record for a stream, made on first mention: by 'sub', or by a drawing's 'attach'
 * that lands before the registry has settled its demand. */
function ensureSlot(node: string, slot: string): SlotState {
	const k = streamKey(node, slot);
	let st = slots.get(k);
	if (!st) {
		const proto = self.location.protocol === 'https:' ? 'wss:' : 'ws:';
		st = {
			node,
			slot,
			ws: null,
			url: dataUrl(proto, self.location.host, node, slot),
			opened: false,
			closed: false,
			reconnectMs: 250,
			specs: [],
			frames: false,
			drawings: new Set(),
			latest: null,
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
				surfaces.set(m.id, createSurface(m.canvas));
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
			if (d.slot !== st) detach(d, false);
			d.slot = st;
			st.drawings.add(d);
			invalidate(d);
			break;
		}
		case 'detach': {
			const d = drawings.get(m.id);
			if (d) detach(d, m.keep);
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

/** Decode one frame and mark what it changes for the next flush: the drawings it feeds, and the
 * news the main thread is owed (the frame where a reader asked, else its head). */
function arrive(st: SlotState, raw: ArrayBuffer): void {
	let frame: DataFrame;
	try {
		const stamps = decodeStamps(raw);
		if (stamps) {
			if (st.latest) st.latest = { ...st.latest, meta: { ...st.latest.meta, ...stamps } };
			const n = st.news;
			if (n?.frame) n.frame = { ...n.frame, meta: { ...n.frame.meta, ...stamps } };
			else if (n?.head) n.head = { ...n.head, meta: { ...n.head.meta, ...stamps } };
			else st.news = { stamps: { ...n?.stamps, ...stamps } };
			requestFlush();
			return;
		}
		frame = decodeData(raw);
	} catch {
		return; // a corrupt frame shouldn't kill the slot
	}
	st.arrivals++;
	st.latest = frame;
	// A frame over one no flush has shown yet is a drop, whoever would have drawn it.
	if (st.unflushed) st.drops++;
	st.unflushed = true;
	for (const d of st.drawings) stale.add(d);
	st.news = st.frames ? { frame } : { head: headOf(frame) };
	requestFlush();
}
