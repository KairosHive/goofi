/** Data-plane transport: the main-thread wire to `dataWorker.ts`, which owns the sockets, the
 * decode and the plot surfaces. Viewer counting belongs to the registry in `frames.ts`; a
 * drawing's life belongs to `drawings.ts`. Neither is decided here. */
import type { ViewSpec } from '$lib/viewers/module';
import type { ToMain, ToWorker } from './dataProtocol';

let worker: Worker | null = null;
const listeners = new Set<(m: ToMain) => void>();

function ensureWorker(): Worker {
	if (worker) return worker;
	worker = new Worker(new URL('./dataWorker.ts', import.meta.url), { type: 'module' });
	worker.addEventListener('message', (e: MessageEvent) => {
		for (const cb of listeners) cb(e.data as ToMain);
	});
	return worker;
}

/** Everything the worker says reaches every listener; each picks out what is its own. */
export function listen(cb: (m: ToMain) => void): () => void {
	listeners.add(cb);
	return () => listeners.delete(cb);
}

export function post(m: ToWorker, transfer: Transferable[] = []): void {
	ensureWorker().postMessage(m, transfer);
}

/** Open the `(node, slot)` stream. Idempotent at the worker: a stream already open stays open.
 * `frames` asks for the decoded frames on this thread, beside whatever the worker draws. */
export function openStream(node: string, slot: string, frames: boolean): void {
	post({ op: 'sub', node, slot, frames });
}

/** Close the `(node, slot)` stream and drop its socket. */
export function closeStream(node: string, slot: string): void {
	post({ op: 'unsub', node, slot });
}

/** Declare the page's display rate on every stream, open and to come. */
export function declareRate(fps: number): void {
	post({ op: 'rate', fps });
}

/** Ask the backend to reduce this stream to `specs` — every bound viewer's constraint, verbatim —
 * and say whether this thread wants the frames. */
export function sendSpecs(node: string, slot: string, specs: ViewSpec[], frames: boolean): void {
	post({ op: 'spec', node, slot, specs, frames });
}
