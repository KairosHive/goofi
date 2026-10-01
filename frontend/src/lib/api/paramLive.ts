import { wsUrl } from './wsUrl';

export interface LiveSource {
	values: Record<string, Record<string, unknown>>;
	errors: Record<string, Record<string, string>>;
}

interface Watch {
	ws: WebSocket | null;
	holders: number;
	retry?: ReturnType<typeof setTimeout>;
}

export class ParamLive {
	private open = new Map<string, Watch>();

	constructor(private readonly apply: (node: string, live: LiveSource) => void) {}

	watch(uid: string): () => void {
		let held = this.open.get(uid);
		if (!held) {
			held = { ws: null, holders: 0 };
			this.open.set(uid, held);
			this.connect(uid, held);
		}
		held.holders += 1;
		let released = false;
		return () => {
			if (released) return;
			released = true;
			held.holders -= 1;
			queueMicrotask(() => {
				if (held.holders > 0 || this.open.get(uid) !== held) return;
				this.open.delete(uid);
				clearTimeout(held.retry);
				held.ws?.close();
			});
		};
	}

	private connect(uid: string, held: Watch): void {
		const ws = new WebSocket(wsUrl(['params', uid]));
		held.ws = ws;
		ws.addEventListener('message', (e: MessageEvent) => {
			if (held.ws === ws && held.holders > 0) this.receive(String(e.data));
		});
		ws.addEventListener('close', () => {
			if (held.ws !== ws) return;
			held.ws = null;
			if (this.open.get(uid) !== held) return;
			held.retry = setTimeout(() => {
				if (held.holders > 0) this.connect(uid, held);
			}, 500);
		});
	}

	private receive(text: string): void {
		const msg = JSON.parse(text) as { node?: string } & Partial<LiveSource>;
		if (!msg.node) return;
		this.apply(msg.node, { values: msg.values ?? {}, errors: msg.errors ?? {} });
	}
}
