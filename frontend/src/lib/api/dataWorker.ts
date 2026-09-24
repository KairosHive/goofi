/** Data-plane Web Worker: one WebSocket per (node, slot), each frame decoded on arrival and
 * posted to the main thread with its array buffer transferred. `frames.ts` owns latest-wins. */
import { decodeData, decodeStamps, type DataFrame } from '$lib/codec/decode';
import { dataUrl } from './dataUrl';
import { streamKey } from './streamKey';

interface SlotState {
	node: string;
	slot: string;
	ws: WebSocket | null;
	url: string;
	closed: boolean;
	reconnectMs: number;
	/** The ViewSpecs every viewer of this slot has contributed; empty means full resolution. */
	specs: unknown[];
}

const slots = new Map<string, SlotState>();
/** The page's display rate, declared with every stream's specs; unset until it is measured. */
let fps: number | undefined;

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
		if (e.data instanceof ArrayBuffer) post(st, e.data);
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

self.addEventListener('message', (e: MessageEvent) => {
	const m = e.data as { op: string; node: string; slot: string; specs?: unknown[]; fps?: number };
	const k = streamKey(m.node, m.slot);
	if (m.op === 'rate') {
		fps = m.fps;
		for (const st of slots.values()) sendSpecs(st);
	} else if (m.op === 'sub') {
		let st = slots.get(k);
		if (!st) {
			const proto = self.location.protocol === 'https:' ? 'wss:' : 'ws:';
			const url = dataUrl(proto, self.location.host, m.node, m.slot);
			st = { node: m.node, slot: m.slot, ws: null, url, closed: false, reconnectMs: 250, specs: [] };
			slots.set(k, st);
			openWs(st);
		}
	} else if (m.op === 'spec') {
		// A 'sub' always precedes a 'spec', so a spec for an absent slot is a post-unsub straggler.
		const st = slots.get(k);
		if (st) {
			st.specs = m.specs ?? [];
			sendSpecs(st);
		}
	} else if (m.op === 'unsub') {
		const st = slots.get(k);
		if (!st) return;
		st.closed = true;
		st.ws?.close();
		slots.delete(k);
	}
});

/** Decode one frame and hand it to the main thread, its buffers transferred; a held frame's
 * stamps go as the small message they are. */
function post(st: SlotState, raw: ArrayBuffer): void {
	let frame: DataFrame;
	try {
		const stamps = decodeStamps(raw);
		if (stamps) {
			(self as unknown as Worker).postMessage({ node: st.node, slot: st.slot, stamps });
			return;
		}
		frame = decodeData(raw);
	} catch {
		return; // a corrupt frame shouldn't kill the slot
	}
	const transfer = new Set<ArrayBufferLike>();
	collectBuffers(frame, transfer);
	(self as unknown as Worker).postMessage(
		{ node: st.node, slot: st.slot, frame },
		Array.from(transfer) as Transferable[]
	);
}
