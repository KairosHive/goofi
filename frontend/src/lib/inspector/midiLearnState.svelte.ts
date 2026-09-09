import { bindViewer } from '$lib/api/frames';
import type { NodeInstanceInfo } from '$lib/api/control';
import type { ArrayData } from '$lib/codec/decode';
import { feeds, type SlotDtype } from '$lib/api/vocab';
import { notify } from '$lib/stores/notify.svelte';

/** One pending MIDI mapping across widgets and parameters. */
class MidiLearn {
	target = $state<string | null>(null);
	node = $state('');
	owner: string | null = null;
	private unbind: (() => void)[] = [];

	stop(): void {
		this.target = null;
		this.node = '';
		this.owner = null;
		for (const release of this.unbind) release();
		this.unbind = [];
	}

	start(owner: string, target: string, node: NodeInstanceInfo, commit: (reference: string, index: number) => void): void {
		this.stop();
		const slots = Object.entries(node.output_slots).filter(([, dtype]) => feeds(dtype as SlotDtype, 'ARRAY'));
		if (!slots.length) {
			notify().raise('This MIDI node has no numeric output to learn.');
			return;
		}
		this.owner = owner;
		this.target = target;
		this.node = node.uid;
		// Keep every element so a learned index addresses the original frame.
		for (const [slot] of slots) {
			let baseline: number[] | null = null;
			this.unbind.push(bindViewer(node.uid, slot, `midi-learn:${target}`, [{ dtype: 'array', ndim: [], dims: [], reduce: [] }], (frame) => {
				if (this.target !== target) return;
				const values = (frame.data as ArrayData).values;
				if (!values) return;
				if (!baseline || baseline.length !== values.length) {
					baseline = Array.from(values);
					return;
				}
				const index = Array.from(values).findIndex((v, i) => Number.isFinite(v) && v !== baseline![i]);
				if (index < 0) return;
				this.stop();
				commit(`${node.name}.${node.slot_labels?.[slot] ?? slot}`, index);
			}));
		}
	}
}

export const midiLearn = new MidiLearn();
