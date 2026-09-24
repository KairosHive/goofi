/** Data-plane transport: the main-thread wire to `dataWorker.ts`. Viewer counting belongs to
 * the registry in `frames.ts`, never here. */
import type { DataFrame } from '$lib/codec/decode';
import type { ViewSpec } from '$lib/viewers/capacity';

/** Where decoded frames go. One sink, registered once by `frames.ts`. */
type FrameSink = (node: string, slot: string, frame: DataFrame) => void;
/** Where a held frame's fresh stamps go: the same registry, which folds them into that frame. */
type StampsSink = (node: string, slot: string, stamps: Record<string, unknown>) => void;

let worker: Worker | null = null;
let sink: FrameSink | null = null;
let stampsSink: StampsSink | null = null;

/** Route decoded frames to `f`. Called once, at `frames.ts` module init. */
export function setFrameSink(f: FrameSink): void {
	sink = f;
}

/** Route a held frame's stamps to `f`. Called once, at `frames.ts` module init. */
export function setStampsSink(f: StampsSink): void {
	stampsSink = f;
}

function ensureWorker(): Worker {
	if (worker) return worker;
	worker = new Worker(new URL('./dataWorker.ts', import.meta.url), { type: 'module' });
	worker.addEventListener('message', (e: MessageEvent) => {
		const m = e.data as { node: string; slot: string; frame?: DataFrame; stamps?: Record<string, unknown> };
		if (m.frame) sink?.(m.node, m.slot, m.frame);
		else if (m.stamps) stampsSink?.(m.node, m.slot, m.stamps);
	});
	return worker;
}

/** Open the `(node, slot)` stream. Idempotent at the worker: a stream already open stays open. */
export function openStream(node: string, slot: string): void {
	ensureWorker().postMessage({ op: 'sub', node, slot });
}

/** Close the `(node, slot)` stream and drop its socket. */
export function closeStream(node: string, slot: string): void {
	ensureWorker().postMessage({ op: 'unsub', node, slot });
}

/** Declare the page's display rate on every stream, open and to come. */
export function declareRate(fps: number): void {
	ensureWorker().postMessage({ op: 'rate', fps });
}

/** Ask the backend to reduce this stream to `specs` — every bound viewer's constraint, verbatim. */
export function sendSpecs(node: string, slot: string, specs: ViewSpec[]): void {
	ensureWorker().postMessage({ op: 'spec', node, slot, specs });
}
