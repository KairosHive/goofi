import { bindViewer } from '$lib/api/frames';
import type { ArrayData } from '$lib/codec/decode';
import { graph } from '$lib/stores/graph.svelte';

/** One pending MIDI mapping across widgets and parameters, listening to the bus: every MIDI
 * group's `cc` and `notes`, read over `/data/variables/<name>` as any value is. */
class MidiLearn {
	target = $state<string | null>(null);
	owner: string | null = null;
	private unbind: (() => void)[] = [];

	stop(): void {
		this.target = null;
		this.owner = null;
		for (const release of this.unbind) release();
		this.unbind = [];
	}

	/** Listen on `groups` until one element moves against its baseline, then commit the variable
	 * that moved, `variables.<group>.<entry>`, and the index that moved in it. */
	start(owner: string, target: string, groups: string[], commit: (reference: string, index: number) => void): void {
		this.stop();
		this.owner = owner;
		this.target = target;
		for (const group of groups) {
			for (const entry of ['cc', 'notes']) {
				const name = `${group}.${entry}`;
				let baseline: number[] | null = null;
				this.unbind.push(bindViewer('variables', name, `midi-learn:${target}:${name}`, null, (frame) => {
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
					commit(`variables.${name}`, index);
				}));
			}
		}
	}

	/** Stop when the group listened to leaves the patch. */
	watch(): void {
		$effect(() => {
			const groups = Object.keys(graph().midiGroups);
			if (this.target !== null && groups.length === 0) this.stop();
		});
	}
}

export const midiLearn = new MidiLearn();
