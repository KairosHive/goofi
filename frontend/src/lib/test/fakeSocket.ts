/** Test double for `WebSocket`: a test opens it, closes it and speaks as the server. Stub the
 * global with the class and read the sockets a client opened off `FakeSocket.opened`. */
export class FakeSocket {
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
	/** One frame from the server. */
	say(frame: unknown): void {
		this.fire('message', typeof frame === 'string' ? frame : JSON.stringify(frame));
	}
}
