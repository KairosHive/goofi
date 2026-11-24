/** Fill a store replica the way the manager does — by emitting `doc_state` / `doc_patch`, never by writing into it. */
import { diffOps, isObj, type Obj } from '$lib/crdt/ops';
import { emptyDoc, type Doc } from '$lib/crdt/graphDoc';
import { SCOPE_TYPE } from '$lib/api/vocab';
import type { FakeControl } from './fakeControl';

export class DocSeed {
	private doc: Doc = emptyDoc();
	private v = 0;

	/** Seeds the empty document at once: a replica refuses a patch until it has a base. */
	constructor(private fc: FakeControl) {
		this.push();
	}

	get version(): number {
		return this.v;
	}

	/** Send the whole document, as a connection or a lag recovery does. */
	push(doc: Doc = this.doc): this {
		this.doc = structuredClone(doc);
		this.v += 1;
		this.fc.emit({ event: 'doc_state', payload: { v: this.v, doc: structuredClone(this.doc) } });
		return this;
	}

	/** Send one change, spelled as a merge — `null` at a key deletes it — and sent as the ops the
	 * manager would send for it. */
	patch(patch: Obj): this {
		const before = structuredClone(this.doc);
		merge(this.doc, patch);
		const from = this.v;
		this.v += 1;
		this.fc.emit({ event: 'doc_patch', payload: { from, v: this.v, ops: diffOps(before, this.doc) } });
		return this;
	}

	/** One node, with whatever leaves the test cares about beyond identity. */
	node(uid: string, type: string, name: string, pos: [number, number] = [0, 0], extra: Obj = {}): this {
		return this.patch({ nodes: { [uid]: { type, name, pos: [pos[0], pos[1]], ...extra } } });
	}

	/** One sub-patch facade — a node record wearing the scope type. Membership is each MEMBER's own
	 * `scope`, so seed the members with it rather than listing them here. */
	instance(uid: string, name: string, pos: [number, number] = [0, 0], extra: Obj = {}): this {
		return this.node(uid, SCOPE_TYPE, name, pos, extra);
	}

	/** One boundary port of `scope`. Its direction and dtype ARE its type; its inner wire is a link. */
	port(uid: string, type: string, name: string, scope: string, pos: [number, number] = [0, 0]): this {
		return this.node(uid, type, name, pos, { scope });
	}

	/** One variable, `{value, type, system}`. */
	variable(name: string, rec: Obj): this {
		return this.patch({ variables: { [name]: rec } });
	}

	remove(root: string, key: string): this {
		return this.patch({ [root]: { [key]: null } });
	}
}

export function seed(fc: FakeControl): DocSeed {
	return new DocSeed(fc);
}

/** A test spells a change as a merge: an object merges, `null` removes, anything else replaces whole. */
function merge(target: Obj, patch: Obj): void {
	for (const [k, pv] of Object.entries(patch)) {
		if (pv === null) delete target[k];
		else if (isObj(pv)) {
			if (!isObj(target[k])) target[k] = {};
			merge(target[k] as Obj, pv);
		} else target[k] = pv;
	}
}
