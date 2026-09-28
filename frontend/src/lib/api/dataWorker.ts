/** Data-plane Web Worker: one WebSocket per (node, slot), each frame decoded on arrival and
 * drawn here on the plot surfaces the main thread handed over; a frame goes to the main thread
 * only where a reader there asked for it. `frames.ts` owns the demand, `drawings.ts` the handles. */
import { decodeData, decodeStamps, isArrayFrame, type DataFrame } from '$lib/codec/decode';
import { createSurface, type Surface } from 'plotluck';
import { BrainDrawing } from '$lib/viewers/brainDrawing';
import { ImageDrawing, LineDrawing, TrajectoryDrawing, type DrawBox, type Drawing, type DrawnState } from '$lib/viewers/drawing';
import type { ProbeBox } from '$lib/viewers/hover';
import { isRenderable, type ViewerKind } from '$lib/viewers/kind';
import type { SettingsMap } from '$lib/viewers/settingsSchema';
import { summaryOf } from '$lib/viewers/viewMeta';
import { headOf, type ToMain, type ToWorker } from './dataProtocol';
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
	/** Since the last stats report: what the wire delivered, and what arrived over an undrawn frame. */
	arrivals: number;
	drops: number;
	/** A frame arrived and no draw has run since. */
	undrawn: boolean;
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

const send = (m: ToMain, transfer: Transferable[] = []): void =>
	(self as unknown as Worker).postMessage(m, transfer);

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

function collectBuffers(frame: DataFrame, out: Set<ArrayBufferLike>): void {
	const d = frame.data as unknown;
	if (frame.dtype === 'ARRAY') {
		const values = (d as { values?: ArrayLike<number> & { buffer?: ArrayBufferLike } }).values;
		if (values?.buffer) out.add(values.buffer);
	} else if (frame.dtype === 'TABLE' && d && typeof d === 'object') {
		for (const v of Object.values(d as Record<string, DataFrame>)) collectBuffers(v, out);
	}
}

// ── paints and stats ─────────────────────────────────────────────────────────────────────────
// One paint per animation frame in which a frame was drawn: the surfaces coalesce the pushes
// the same way, so this is how often the page's pictures changed.
const schedule =
	typeof requestAnimationFrame === 'function'
		? requestAnimationFrame
		: (fn: () => void): number => setTimeout(fn, 16) as unknown as number;
let paintScheduled = false;
let paints = 0;
function paint(): void {
	if (paintScheduled) return;
	paintScheduled = true;
	schedule(() => {
		paintScheduled = false;
		paints++;
		for (const st of slots.values()) st.undrawn = false;
	});
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

function make(surface: Surface, kind: ViewerKind, trajectory: boolean): Drawing {
	if (kind === 'brain') return new BrainDrawing(surface);
	if (kind === 'image') return new ImageDrawing(surface);
	return trajectory ? new TrajectoryDrawing(surface) : new LineDrawing(surface);
}

function detach(d: DrawingState, keep: boolean): void {
	d.slot?.drawings.delete(d);
	d.slot = null;
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
			arrivals: 0,
			drops: 0,
			undrawn: false
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
				drawing: make(surface, m.kind, m.trajectory),
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
			// A settings change redraws the last frame under the new settings.
			if (d.slot?.latest) render(d, d.slot.latest);
			break;
		}
		case 'attach': {
			const d = drawings.get(m.id);
			if (!d) break;
			const st = ensureSlot(m.node, m.slot);
			if (d.slot !== st) detach(d, false);
			d.slot = st;
			st.drawings.add(d);
			if (st.latest) render(d, st.latest);
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

/** Decode one frame, draw it where it is wanted, and hand it on: a held frame's stamps go as
 * the small message they are; the frame itself goes to the main thread only where a reader
 * asked, its buffers transferred unless a drawing here still holds them. */
function arrive(st: SlotState, raw: ArrayBuffer): void {
	let frame: DataFrame;
	try {
		const stamps = decodeStamps(raw);
		if (stamps) {
			if (st.latest) st.latest = { ...st.latest, meta: { ...st.latest.meta, ...stamps } };
			send({ node: st.node, slot: st.slot, stamps });
			return;
		}
		frame = decodeData(raw);
	} catch {
		return; // a corrupt frame shouldn't kill the slot
	}
	st.arrivals++;
	st.latest = frame;
	if (st.drawings.size > 0) {
		// A frame over one no draw has shown yet is a drop; a reader's own coalescing is its own.
		if (st.undrawn) st.drops++;
		st.undrawn = true;
		for (const d of st.drawings) render(d, frame);
		paint();
	}
	if (!st.frames) {
		send({ node: st.node, slot: st.slot, head: headOf(frame) });
		return;
	}
	const transfer = new Set<ArrayBufferLike>();
	if (st.drawings.size === 0) collectBuffers(frame, transfer);
	send({ node: st.node, slot: st.slot, frame }, Array.from(transfer) as Transferable[]);
}
