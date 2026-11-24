/** Data-plane transport: the main-thread wire to `dataWorker.ts`, which owns the sockets, the
 * decode and the plot surfaces. */
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
