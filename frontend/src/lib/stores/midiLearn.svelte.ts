import { bindViewer } from '$lib/api/frames';
import type { ViewSpec } from '$lib/viewers/module';
import type { ArrayData } from '$lib/codec/decode';
import { graph } from '$lib/stores/graph.svelte';

/** One pending MIDI mapping across widgets and parameters. A learn opens every port the host
 * lists, as the manager's groups, and listens to each entry over `/data/variables/<name>` as any
 * value is read; the manager closes what nothing reads when the learn ends. */
/** Every element of an entry, whole: an index is a controller, so no preview may stand in. */
const WHOLE: ViewSpec[] = [{ dtype: 'array', ndim: [['eq', 1]], reduce: [{ dim: 0, max: 'whole' }] }];

class MidiLearn {
	target = $state<string | null>(null);
	owner: string | null = null;
	private groups: string[] = [];
	private seen = new Set<string>();
	private unbind: (() => void)[] = [];
	private timer: ReturnType<typeof setTimeout> | null = null;

	stop(): void {
		const was = this.target !== null;
		this.target = null;
		this.owner = null;
		this.groups = [];
		this.seen.clear();
		for (const release of this.unbind) release();
		this.unbind = [];
		if (this.timer) clearTimeout(this.timer);
		this.timer = null;
		if (was) void graph().learnMidi(false).catch(() => {});
	}

	/** Open every port and listen until one element moves against its baseline, then commit the
	 * variable that moved, `variables.<group>.<entry>`, and the index that moved in it. Answers
	 * how many devices listen; none is told to the caller, not to the manager. */
	async start(owner: string, target: string, commit: (reference: string, index: number) => void): Promise<number> {
		this.stop();
		const { groups, seconds } = await graph().learnMidi(true);
		if (groups.length === 0) return 0;
		this.owner = owner;
		this.target = target;
		this.groups = groups.map((g) => g.group);
		this.timer = setTimeout(() => this.stop(), seconds * 1000);
		for (const group of this.groups) {
			for (const entry of ['cc', 'notes', 'bend', 'pressure']) {
				const name = `${group}.${entry}`;
				let baseline: number[] | null = null;
				this.unbind.push(bindViewer('variables', name, `midi-learn:${target}:${name}`, WHOLE, (frame) => {
					if (this.target !== target) return;
					const values = (frame.data as ArrayData).values;
					if (!values) return;
					if (!baseline || baseline.length !== values.length) {
						baseline = Array.from(values);
						return;
					}
					const index = Array.from(values).findIndex((v, i) => Number.isFinite(v) && v !== baseline![i]);
					if (index < 0) return;
					// The mapping lands before the learn ends, so the device it names stays open.
					commit(`variables.${name}`, index);
					this.stop();
				}));
			}
		}
		return groups.length;
	}

	/** Stop once every device listened to has left the bus. The replica learns of a group after
	 * the learn's reply names it, so a group counts only once the replica has shown it. */
	watch(): void {
		$effect(() => {
			const present = graph().midiGroups;
			for (const g of this.groups) if (present[g]) this.seen.add(g);
			if (this.target !== null && this.seen.size > 0 && this.groups.every((g) => !present[g])) this.stop();
		});
	}
}

export const midiLearn = new MidiLearn();
