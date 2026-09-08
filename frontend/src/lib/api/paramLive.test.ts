import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { paramsUrl, ParamLive, type LiveSource } from './paramLive';

/** A stand-in for the browser's `WebSocket`, CONNECTING at birth as a real one is. */
class FakeSocket {
	static all: FakeSocket[] = [];
	readyState = 0;
	closed = false;
	private listeners: Record<string, ((e: unknown) => void)[]> = {};
	constructor(readonly url: string) {
		FakeSocket.all.push(this);
	}
	addEventListener(t: string, cb: (e: unknown) => void): void {
		(this.listeners[t] ??= []).push(cb);
	}
	close(): void {
		this.closed = true;
		this.readyState = 3;
	}
	deliver(data: unknown): void {
		for (const cb of this.listeners.message ?? []) cb({ data });
	}
}

beforeEach(() => {
	FakeSocket.all = [];
	vi.stubGlobal('WebSocket', FakeSocket as unknown as typeof WebSocket);
	vi.stubGlobal('location', { protocol: 'http:', host: 'here:8000' });
});
afterEach(() => vi.unstubAllGlobals());

const seen: [string, LiveSource][] = [];
const live = (): ParamLive => {
	seen.length = 0;
	return new ParamLive((node, l) => seen.push([node, l]));
};

describe('the live param plane', () => {
	it('addresses one node', () => {
		expect(paramsUrl('ws:', 'localhost:8000', '000000000001')).toBe(
			'ws://localhost:8000/params/000000000001'
		);
	});

	it('opens one socket per node and hands each frame to the store', () => {
		const p = live();
		p.watch('n1');
		expect(FakeSocket.all.map((s) => s.url)).toEqual(['ws://here:8000/params/n1']);
		FakeSocket.all[0].deliver(
			JSON.stringify({ node: 'n1', values: { lfo: { frequency: 3 } }, errors: {} })
		);
		expect(seen).toEqual([['n1', { values: { lfo: { frequency: 3 } }, errors: {} }]]);
	});

	// Two inspectors can show one node — a second editor's pane, a Parameters panel — and the one
	// that closes first must not take the other's readout with it.
	it('shares one socket between holders, and closes it when the last one lets go', () => {
		const p = live();
		const a = p.watch('n1');
		const b = p.watch('n1');
		expect(FakeSocket.all.length, 'one socket, two holders').toBe(1);
		a();
		expect(FakeSocket.all[0].closed, 'the other holder still wants it').toBe(false);
		b();
		expect(FakeSocket.all[0].closed).toBe(true);
	});

	it('opens a fresh socket after the last holder let go', () => {
		const p = live();
		p.watch('n1')();
		p.watch('n1');
		expect(FakeSocket.all.length).toBe(2);
		expect(FakeSocket.all[1].closed).toBe(false);
	});

	it('ignores a frame that names no node, rather than writing onto one', () => {
		const p = live();
		p.watch('n1');
		FakeSocket.all[0].deliver(JSON.stringify({ values: {}, errors: {} }));
		expect(seen).toEqual([]);
	});

	// A frame is BOTH maps or neither: an absent one reads as empty, never as "unchanged", or a
	// param whose error cleared would keep showing it.
	it('reads an absent map as empty', () => {
		const p = live();
		p.watch('n1');
		FakeSocket.all[0].deliver(JSON.stringify({ node: 'n1' }));
		expect(seen).toEqual([['n1', { values: {}, errors: {} }]]);
	});
});
