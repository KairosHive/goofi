/** The document driver: follows `doc_state` / `doc_patch` so the replica equals the manager's
 * document. A delta writes only the records it names, so only their readers re-run. */
import type { Control } from '$lib/api/control';
import { applyOps, type Op } from './ops';
import { emptyDoc, type Doc } from './graphDoc';

export class SyncClient {
	private _doc = $state<Doc>(emptyDoc());
	get doc(): Doc {
		return this._doc;
	}
	/** Told what moved: the applied ops, or `null` when the whole document was replaced. */
	private changed: (ops: Op[] | null) => void;
	/** The version `_doc` is at, or `-1` before the first `doc_state`. */
	private _version = $state(-1);
	get version(): number {
		return this._version;
	}
	/** Whether this replica has been seeded yet — until it flips, reads describe an empty replica. */
	get synced(): boolean {
		return this._version >= 0;
	}

	constructor(control: Control, changed: (ops: Op[] | null) => void) {
		this.changed = changed;
		control.on((ev) => {
			if (ev.event === 'doc_state') {
				this._doc = ev.payload.doc;
				this._version = ev.payload.v;
				this.changed(null);
			} else if (ev.event === 'doc_patch') {
				this.applyPatch(ev.payload.from, ev.payload.v, ev.payload.ops);
			}
		});
	}

	/** Drop the replica to empty for a NEW backend session, whose engine remints uids from 1. */
	reset(): void {
		this._doc = emptyDoc();
		this._version = -1;
		this.changed(null);
	}

	/** Apply one delta: one already held is skipped, one reaching past this version is refused. */
	applyPatch(from: number, to: number, ops: Op[]): void {
		if (to <= this._version) return; // stale: the seed already carries it
		if (from !== this._version) {
			console.warn(
				`goofi: doc patch spans v${from}→v${to} but this replica is at v${this._version} — a delta was lost; waiting for a fresh doc_state`
			);
			return;
		}
		applyOps(this._doc, ops);
		this._version = to;
		this.changed(ops);
	}
}
