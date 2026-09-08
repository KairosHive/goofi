/** The live param plane: one `/params/<uid>` socket per WATCHED node, ref-counted so several
 *  inspectors on one node share it. The control plane carries the same pairs on its own slower
 *  clock, so a socket that never opens costs smoothness and never correctness. */

export interface LiveSource {
	values: Record<string, Record<string, unknown>>;
	errors: Record<string, Record<string, string>>;
}

/** Where one node's live params come from. */
export function paramsUrl(proto: string, host: string, node: string): string {
	return `${proto}//${host}/params/${encodeURIComponent(node)}`;
}

interface Watch {
	ws: WebSocket;
	holders: number;
}

export class ParamLive {
	private open = new Map<string, Watch>();

	constructor(private readonly apply: (node: string, live: LiveSource) => void) {}

	/** Keep `uid`'s params live until the returned undo runs. */
	watch(uid: string): () => void {
		const held = this.open.get(uid);
		if (held) {
			held.holders += 1;
		} else {
			const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
			const ws = new WebSocket(paramsUrl(proto, location.host, uid));
			ws.addEventListener('message', (e: MessageEvent) => this.receive(String(e.data)));
			this.open.set(uid, { ws, holders: 1 });
		}
		return () => this.release(uid);
	}

	private release(uid: string): void {
		const w = this.open.get(uid);
		if (!w) return;
		w.holders -= 1;
		if (w.holders > 0) return;
		this.open.delete(uid);
		w.ws.close();
	}

	// The frame names its own node: a socket is per node, but the payload is the same shape the
	// control plane sends, so ONE reader serves both.
	private receive(text: string): void {
		const msg = JSON.parse(text) as { node?: string } & Partial<LiveSource>;
		if (!msg.node) return;
		this.apply(msg.node, { values: msg.values ?? {}, errors: msg.errors ?? {} });
	}
}
