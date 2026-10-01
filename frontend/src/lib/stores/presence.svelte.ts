/** Who else is in this patch, and where their pointers are: one `/presence` socket per tab. Pointers
 * are window fractions, so a peer's point lands in the same panel on any screen. */
import { workspace } from 'panelty';
import { wsUrl } from '$lib/api/wsUrl';

export interface Peer {
	id: number;
	hue: number;
}

export interface Cursor extends Peer {
	tab: string;
	x: number;
	y: number;
}

export class PresenceStore {
	/** Every browser in the patch, this one included. */
	peers = $state.raw<Peer[]>([]);
	/** The other peers' last pointer, by id; absent once a pointer left the window. */
	cursors = $state.raw<Record<number, Cursor>>({});
	private me: number | null = null;
	private ws: WebSocket | null = null;
	private retry: ReturnType<typeof setTimeout> | null = null;
	private frame = 0;
	private next: { x: number; y: number } | null | undefined;
	private stopped = false;

	/** The peers on this tab, beside this one. */
	get others(): Cursor[] {
		const tab = workspace().active.id;
		return Object.values(this.cursors).filter((c) => c.id !== this.me && c.tab === tab);
	}

	/** Open the socket and follow the pointer; the returned function leaves. */
	start(): () => void {
		this.stopped = false;
		this.connect();
		const move = (e: PointerEvent) => this.queue({ x: e.clientX / window.innerWidth, y: e.clientY / window.innerHeight });
		const leave = () => this.queue(null);
		window.addEventListener('pointermove', move);
		document.addEventListener('pointerleave', leave);
		window.addEventListener('blur', leave);
		return () => {
			this.stopped = true;
			window.removeEventListener('pointermove', move);
			document.removeEventListener('pointerleave', leave);
			window.removeEventListener('blur', leave);
			if (this.retry) clearTimeout(this.retry);
			cancelAnimationFrame(this.frame);
			this.ws?.close();
			this.ws = null;
			this.peers = [];
			this.cursors = {};
		};
	}

	/** The newest point wins; one message per animation frame. */
	private queue(at: { x: number; y: number } | null): void {
		this.next = at;
		if (this.frame) return;
		this.frame = requestAnimationFrame(() => {
			this.frame = 0;
			if (this.next === undefined || this.ws?.readyState !== WebSocket.OPEN) return;
			const tab = workspace().active.id;
			this.ws.send(JSON.stringify(this.next ? { tab, ...this.next } : { leave: true }));
			this.next = undefined;
		});
	}

	private connect(): void {
		const ws = new WebSocket(wsUrl(['presence']));
		this.ws = ws;
		ws.addEventListener('message', (e: MessageEvent) => this.receive(String(e.data)));
		ws.addEventListener('close', () => {
			if (this.ws !== ws) return;
			this.ws = null;
			this.peers = [];
			this.cursors = {};
			if (!this.stopped) this.retry = setTimeout(() => this.connect(), 1000);
		});
	}

	receive(text: string): void {
		const m = JSON.parse(text) as { you?: number; peers?: Peer[]; cursor?: Cursor; gone?: number };
		if (m.you !== undefined) this.me = m.you;
		if (m.peers) this.peers = m.peers;
		if (m.cursor && m.cursor.id !== this.me) this.cursors = { ...this.cursors, [m.cursor.id]: m.cursor };
		if (m.gone !== undefined) {
			const { [m.gone]: _, ...rest } = this.cursors;
			this.cursors = rest;
		}
	}
}

let store: PresenceStore | null = null;
export function presence(): PresenceStore {
	return (store ??= new PresenceStore());
}
