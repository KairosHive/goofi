import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ControlClient, PROTOCOL_VERSION, historyFeed, type ControlEvent } from './control';

/** The socket a client opens: a test opens it, closes it and speaks as the manager. */
class FakeSocket {
	static readonly OPEN = 1;
	static opened: FakeSocket[] = [];
	readyState = 0;
	/** Every frame the client sent, parsed, in order. */
	sent: Array<Record<string, unknown>> = [];
	private listeners = new Map<string, Array<(e: { data: string }) => void>>();

	constructor(readonly url: string) {
		FakeSocket.opened.push(this);
	}
	addEventListener(type: string, cb: (e: { data: string }) => void): void {
		this.listeners.set(type, [...(this.listeners.get(type) ?? []), cb]);
	}
	send(text: string): void {
		this.sent.push(JSON.parse(text));
	}
	private fire(type: string, data = ''): void {
		for (const cb of this.listeners.get(type) ?? []) cb({ data });
	}
	open(): void {
		this.readyState = FakeSocket.OPEN;
		this.fire('open');
	}
	close(): void {
		this.readyState = 3;
		this.fire('close');
	}
	/** One frame from the manager. */
	say(frame: unknown): void {
		this.fire('message', typeof frame === 'string' ? frame : JSON.stringify(frame));
	}
}

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
	const store = new Map<string, string>();
	vi.stubGlobal('sessionStorage', {
		getItem: (k: string) => store.get(k) ?? null,
		setItem: (k: string, v: string) => void store.set(k, v)
	});
	vi.stubGlobal('WebSocket', FakeSocket);
	vi.stubGlobal('requestAnimationFrame', (cb: () => void) => frames.push(cb));
	vi.stubGlobal('cancelAnimationFrame', () => (frames = []));
});
afterEach(() => {
	vi.unstubAllGlobals();
	vi.restoreAllMocks();
	vi.useRealTimers();
});

/** A client and its socket, open. */
function connected(): { client: ControlClient; ws: FakeSocket } {
	const client = new ControlClient('ws://test/control');
	client.connect();
	const ws = FakeSocket.opened.at(-1)!;
	ws.open();
	return { client, ws };
}

describe('the actor of a tab', () => {
	it('is minted once and is the same for every client of the tab', () => {
		// The manager keeps undo and redo per actor. A client that sent none would share the
		// bridge's `default` actor, and one tab's undo would take back the edit of another.
		const a = new ControlClient('ws://test/control');
		expect(a.actor).toBeTruthy();
		expect(a.actor).not.toBe('default');
		expect(new ControlClient('ws://test/control').actor).toBe(a.actor);
	});
});

describe('the protocol version of a hello', () => {
	/** Whether a client latches a mismatch from a `hello` that reports `version`. */
	function mismatchOn(version: unknown): boolean {
		const { client, ws } = connected();
		ws.say({ event: 'hello', payload: { protocol_version: version } });
		let latched = false;
		client.onProtocolMismatch(() => (latched = true));
		return latched;
	}

	it('accepts an exact match', () => {
		expect(mismatchOn(PROTOCOL_VERSION)).toBe(false);
	});

	it('rejects a newer or older backend version', () => {
		expect(mismatchOn(PROTOCOL_VERSION + 1)).toBe(true);
		expect(mismatchOn(PROTOCOL_VERSION - 1)).toBe(true);
	});

	it('rejects an absent or non-numeric version', () => {
		expect(mismatchOn(undefined)).toBe(true);
		expect(mismatchOn(null)).toBe(true);
		expect(mismatchOn('1')).toBe(true);
	});

	it('stays latched when a later hello matches', () => {
		const { client, ws } = connected();
		ws.say({ event: 'hello', payload: { protocol_version: PROTOCOL_VERSION + 1 } });
		ws.say({ event: 'hello', payload: { protocol_version: PROTOCOL_VERSION } });
		let latched = false;
		client.onProtocolMismatch(() => (latched = true));
		expect(latched).toBe(true);
	});
});

describe('a call and its reply', () => {
	it('sends the op with its id, payload, actor and step, and resolves to the result of that id', async () => {
		const { client, ws } = connected();
		const first = client.call('node remove', { node: 'a' }, { group: 'g1', label: 'Delete' });
		const second = client.call('undo');

		expect(ws.sent).toEqual([
			{ id: 1, op: 'node remove', payload: { node: 'a' }, actor: client.actor, context: null, group: 'g1', label: 'Delete' },
			{ id: 2, op: 'undo', payload: {}, actor: client.actor, context: null }
		]);

		// The replies come back in the other order: each call takes the reply with its id.
		ws.say({ id: 2, result: { changed: true } });
		ws.say({ id: 1, result: 'gone' });
		expect(await second).toEqual({ changed: true });
		expect(await first).toBe('gone');
	});

	it('rejects with the text of an error reply', async () => {
		const { client, ws } = connected();
		const call = client.call('node remove', { node: 'ghost' });
		ws.say({ id: 1, error: 'no node `ghost`' });
		await expect(call).rejects.toThrow('no node `ghost`');
	});

	it('reports the history labels a reply carries', async () => {
		const labels = vi.spyOn(historyFeed, 'labels');
		const { client, ws } = connected();
		const call = client.call('undo');
		ws.say({ id: 1, result: null, history: { undo: null, redo: 'Delete' } });
		await call;
		expect(labels.mock.calls).toEqual([[{ undo: null, redo: 'Delete' }]]);
	});

	it('passes over a frame that is no JSON, no object, or the reply to no call', async () => {
		const { client, ws } = connected();
		const call = client.call('undo');
		ws.say('not json');
		ws.say('7');
		ws.say({ id: 99, result: 'for nobody' });
		ws.say({ id: 1, result: 'mine' });
		expect(await call).toBe('mine');
	});

	it('refuses a call before the socket is open, and sends nothing', async () => {
		const client = new ControlClient('ws://test/control');
		await expect(client.call('undo')).rejects.toThrow('control socket not connected');
		client.connect();
		await expect(client.call('undo')).rejects.toThrow('control socket not connected');
		expect(FakeSocket.opened[0].sent).toEqual([]);
	});
});

describe('a socket that drops', () => {
	it('fails every call in flight and tells the connection listeners', async () => {
		const { client, ws } = connected();
		const states: boolean[] = [];
		client.onConnect((c) => states.push(c));
		const a = client.call('undo');
		const b = client.call('redo');

		ws.close();

		await expect(a).rejects.toThrow('control socket closed');
		await expect(b).rejects.toThrow('control socket closed');
		// The listener hears the state at once, then the one change.
		expect(states).toEqual([true, false]);
	});

	it('opens a fresh socket after a wait that doubles to a cap, and starts over once one opens', () => {
		vi.useFakeTimers();
		const { ws } = connected();
		let last = ws;
		/** The wait until the client opens its next socket after `last` closes, in ms. */
		const waitAfterClose = (): number => {
			last.close();
			const before = FakeSocket.opened.length;
			let waited = 0;
			while (FakeSocket.opened.length === before) {
				vi.advanceTimersByTime(250);
				waited += 250;
				if (waited > 60_000) throw new Error('the client opened no socket');
			}
			last = FakeSocket.opened.at(-1)!;
			return waited;
		};

		// No socket opens in between: the wait grows and stops at the cap.
		expect([1, 2, 3, 4, 5, 6, 7].map(waitAfterClose)).toEqual([250, 500, 1000, 2000, 4000, 5000, 5000]);
		// One that opens resets the wait.
		last.open();
		expect(waitAfterClose()).toBe(250);
	});

	it('`resync` closes the socket, and the next one serves calls again', async () => {
		vi.useFakeTimers();
		const { client, ws } = connected();
		const lost = client.call('undo');

		client.resync();
		await expect(lost).rejects.toThrow('control socket closed');
		vi.advanceTimersByTime(250);
		const next = FakeSocket.opened.at(-1)!;
		expect(next).not.toBe(ws);
		next.open();

		const call = client.call('redo');
		next.say({ id: next.sent[0].id, result: 'again' });
		expect(await call).toBe('again');
	});
});

describe('an event', () => {
	const moved: ControlEvent = { event: 'node_added', payload: { uid: 'n1' } };

	it('reaches every handler, and a handler that left hears no more', () => {
		const { client, ws } = connected();
		const a: ControlEvent[] = [];
		const b: ControlEvent[] = [];
		const leave = client.on((ev) => a.push(ev));
		client.on((ev) => b.push(ev));

		ws.say(moved);
		leave();
		ws.say(moved);

		expect(a).toEqual([moved]);
		expect(b).toEqual([moved, moved]);
	});

	it('still reaches the other handlers when one of them throws', () => {
		const logged = vi.spyOn(console, 'error').mockImplementation(() => {});
		const { client, ws } = connected();
		const heard: ControlEvent[] = [];
		client.on(() => {
			throw new Error('a broken panel');
		});
		client.on((ev) => heard.push(ev));

		ws.say(moved);

		expect(heard).toEqual([moved]);
		expect(logged).toHaveBeenCalledTimes(1);
	});
});

describe('a preview', () => {
	it('sends the newest of each key once per frame, with no id', () => {
		const { client, ws } = connected();
		client.preview('gain', 'node param edit', { value: 1 });
		client.preview('pan', 'node param edit', { value: -1 });
		client.preview('gain', 'node param edit', { value: 2 });
		expect(ws.sent, 'nothing goes out before the frame').toEqual([]);

		runFrame();

		expect(ws.sent).toEqual([
			{ op: 'node param edit', payload: { value: 2 }, actor: client.actor, preview: 'gain' },
			{ op: 'node param edit', payload: { value: -1 }, actor: client.actor, preview: 'pan' }
		]);
		runFrame();
		expect(ws.sent, 'a frame with no new preview sends nothing').toHaveLength(2);
	});

	it('goes out before the call that ends its drag, and not a second time', () => {
		const { client, ws } = connected();
		client.preview('gain', 'node param edit', { value: 3 });
		void client.call('node param edit', { value: 3 });
		runFrame();

		expect(ws.sent.map((f) => ('preview' in f ? 'preview' : 'call'))).toEqual(['preview', 'call']);
	});

	it('is dropped while the socket is closed, and is not sent late', () => {
		vi.useFakeTimers();
		const { client, ws } = connected();
		ws.close();
		client.preview('gain', 'node param edit', { value: 1 });
		runFrame();

		vi.advanceTimersByTime(250);
		const next = FakeSocket.opened.at(-1)!;
		next.open();
		client.preview('pan', 'node param edit', { value: 5 });
		runFrame();
		expect(next.sent).toEqual([
			{ op: 'node param edit', payload: { value: 5 }, actor: client.actor, preview: 'pan' }
		]);
	});
});
