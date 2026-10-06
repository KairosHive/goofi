import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { encode } from '@msgpack/msgpack';
import { beforeAll, describe, expect, it, vi } from 'vitest';
import { STAMPS_TAG } from '$lib/codec/frame';

/** A socket stand-in: records what the worker sends, and delivers what a test hands it. */
class MockSocket {
	static last: MockSocket;
	readyState = 1;
	sent: unknown[] = [];
	private listeners: Record<string, ((e: unknown) => void)[]> = {};
	constructor(readonly url: string) {
		MockSocket.last = this;
	}
	send(text: string): void {
		this.sent.push(JSON.parse(text));
	}
	addEventListener(type: string, cb: (e: unknown) => void): void {
		(this.listeners[type] ??= []).push(cb);
	}
	fire(type: string, e: unknown = {}): void {
		for (const cb of this.listeners[type] ?? []) cb(e);
	}
	close(): void {}
}

const posted: { batch?: { node: string; slot: string; frame?: unknown; stamps?: unknown; head?: unknown }[] }[] = [];
let inbound: (e: { data: unknown }) => void;

beforeAll(async () => {
	vi.stubGlobal('WebSocket', Object.assign(MockSocket, { OPEN: 1 }));
	vi.stubGlobal('self', {
		location: { protocol: 'http:', host: 'localhost:8000' },
		addEventListener: (_: string, cb: typeof inbound) => (inbound = cb),
		postMessage: (m: (typeof posted)[number]) => posted.push(m)
	});
	await import('./dataWorker');
});

const golden = JSON.parse(
	readFileSync(fileURLToPath(new URL('../../../../tests/codec_golden.json', import.meta.url)), 'utf-8')
) as { entries: { name: string; hex: string }[] };
const bytes = (hex: string): ArrayBuffer => Uint8Array.from(hex.match(/../g)!, (b) => parseInt(b, 16)).buffer;

/** A stamps frame: the GOOF header under the stamps tag, its meta, and no body. */
function stampsFrame(meta: Record<string, unknown>): ArrayBuffer {
	const m = encode(meta);
	const out = new Uint8Array(14 + m.length);
	out.set([0x47, 0x4f, 0x4f, 0x46, 2, STAMPS_TAG]);
	new DataView(out.buffer).setUint32(6, m.length, true);
	out.set(m, 14);
	return out.buffer;
}

describe('the data worker', () => {
	it('declares specs and the display rate, and posts what each frame decodes in one batch per flush', () => {
		vi.useFakeTimers();
		inbound({ data: { op: 'sub', node: 'n', slot: 'out', frames: true } });
		const ws = MockSocket.last;
		ws.fire('open');
		expect(ws.sent.at(-1)).toEqual({ op: 'view', specs: [] });
		inbound({ data: { op: 'rate', fps: 60 } });
		expect(ws.sent.at(-1), 'a rate reaches every open stream').toEqual({ op: 'view', specs: [], fps: 60 });

		const frame = bytes(golden.entries.find((e) => e.name === 'array_f32_1d')!.hex);
		ws.fire('message', { data: frame });
		// Stamps that land before the flush ride the pending frame; nothing goes out until it.
		ws.fire('message', { data: stampsFrame({ time: 2 }) });
		expect(posted.length, 'nothing crosses before the flush').toBe(0);
		vi.advanceTimersByTime(20);
		expect(posted.at(-1)).toMatchObject({ batch: [{ node: 'n', slot: 'out', frame: { dtype: 'ARRAY', meta: { time: 2 } } }] });
		ws.fire('message', { data: stampsFrame({ time: 3 }) });
		vi.advanceTimersByTime(20);
		expect(posted.at(-1)).toEqual({ batch: [{ node: 'n', slot: 'out', stamps: { time: 3 } }] });
		const count = posted.length;
		ws.fire('message', { data: new Uint8Array([1, 2, 3]).buffer });
		vi.advanceTimersByTime(20);
		expect(posted.length, 'a corrupt frame posts nothing and keeps the slot').toBe(count);

		// A stream nobody reads on the main thread sends its head there, never the frame.
		inbound({ data: { op: 'spec', node: 'n', slot: 'out', specs: [], frames: false } });
		ws.fire('message', { data: bytes(golden.entries.find((e) => e.name === 'array_f32_1d')!.hex) });
		vi.advanceTimersByTime(20);
		const last = posted.at(-1)!.batch![0] as { frame?: unknown; head?: { dtype: string; array?: { shape: number[] } } };
		expect(last.frame).toBeUndefined();
		expect(last.head?.dtype).toBe('ARRAY');
		expect(last.head?.array?.shape.length).toBe(1);
		vi.useRealTimers();
	});
});
