import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { workspace } from 'panelty';
import { FakeSocket } from '$lib/test/fakeSocket';
import { PresenceStore } from './presence.svelte';

/** The animation frames asked for and not yet run. */
let frames: Array<() => void> = [];
const runFrame = (): void => {
	const due = frames;
	frames = [];
	for (const cb of due) cb();
};

beforeEach(() => {
	FakeSocket.opened = [];
	frames = [];
	vi.stubGlobal('WebSocket', FakeSocket);
	vi.stubGlobal('requestAnimationFrame', (cb: () => void) => frames.push(cb));
	vi.stubGlobal('cancelAnimationFrame', () => (frames = []));
});
afterEach(() => {
	for (const stop of leaving.splice(0)) stop();
	vi.unstubAllGlobals();
	vi.useRealTimers();
});

/** Every store a test started leaves when the test ends, so no listener outlives its test. */
const leaving: Array<() => void> = [];

/** A store that follows the pointer, its open socket, and the function that leaves. */
function started(): { store: PresenceStore; ws: FakeSocket; stop: () => void } {
	const store = new PresenceStore();
	const stop = store.start();
	leaving.push(stop);
	const ws = FakeSocket.opened.at(-1)!;
	ws.open();
	return { store, ws, stop };
}

/** Fake the reconnect timer alone: the animation frames stay the ones a test runs by hand. */
const fakeTimeouts = (): void => void vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });

const pointerAt = (x: number, y: number): void => {
	window.dispatchEvent(new MouseEvent('pointermove', { clientX: x, clientY: y }));
};

describe('who else is in the patch', () => {
	it('keeps the roster whole and the pointers of the other peers on this tab', () => {
		const { store, ws } = started();
		const tab = workspace().active.id;
		ws.say({ you: 1, hue: 10 });
		ws.say({ peers: [{ id: 1, hue: 10 }, { id: 2, hue: 200 }, { id: 3, hue: 90 }] });
		expect(store.peers.map((p) => p.id)).toEqual([1, 2, 3]);

		const here = { id: 2, hue: 200, tab, x: 0.25, y: 0.5 };
		ws.say({ cursor: here });
		ws.say({ cursor: { id: 3, hue: 90, tab: 'another-tab', x: 0.1, y: 0.1 } });
		// The server sends a pointer to every peer. This peer does not draw its own.
		ws.say({ cursor: { id: 1, hue: 10, tab, x: 0.9, y: 0.9 } });
		expect(store.others).toEqual([here]);

		// A peer has one pointer: the newest.
		const moved = { ...here, x: 0.75 };
		ws.say({ cursor: moved });
		expect(store.others).toEqual([moved]);

		ws.say({ gone: 2 });
		expect(store.others).toEqual([]);
		expect(Object.keys(store.cursors)).toEqual(['3']);
	});

	it('sends the newest pointer once per frame as fractions of the window, and a leave', () => {
		const { ws } = started();
		const tab = workspace().active.id;

		pointerAt(window.innerWidth / 2, window.innerHeight / 4);
		pointerAt(window.innerWidth / 4, window.innerHeight / 2);
		expect(ws.sent, 'nothing goes out before the frame').toEqual([]);
		runFrame();
		expect(ws.sent).toEqual([{ tab, x: 0.25, y: 0.5 }]);

		window.dispatchEvent(new Event('blur'));
		runFrame();
		expect(ws.sent.at(-1)).toEqual({ leave: true });
	});

	it('forgets everyone when the socket drops, and opens a new one', () => {
		fakeTimeouts();
		const { store, ws } = started();
		ws.say({ you: 1, hue: 10 });
		ws.say({ peers: [{ id: 1, hue: 10 }, { id: 2, hue: 200 }] });
		ws.say({ cursor: { id: 2, hue: 200, tab: workspace().active.id, x: 0, y: 0 } });

		ws.close();

		expect(store.peers).toEqual([]);
		expect(store.cursors).toEqual({});
		vi.runOnlyPendingTimers();
		expect(FakeSocket.opened).toHaveLength(2);
	});

	it('is silent after it leaves: no socket, no roster, no pointer sent', () => {
		fakeTimeouts();
		const { store, ws, stop } = started();
		ws.say({ peers: [{ id: 1, hue: 10 }] });

		stop();

		expect(ws.readyState, 'the socket is closed').toBe(3);
		expect(store.peers).toEqual([]);
		vi.runOnlyPendingTimers();
		expect(FakeSocket.opened, 'no socket opens after the leave').toHaveLength(1);
		pointerAt(10, 10);
		expect(frames, 'the pointer is not followed any more').toEqual([]);
	});
});
