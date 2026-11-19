<!-- What a track's lane shows across its axis, as a range bar beside the lane: the keys of a note
     track's keyboard, from its lowest to its highest, and the values of a curve track, from its
     least to its most. A drag of the bar moves the range and a drag of an end changes it, shown
     as it goes and one edit at the release; a double click fits it to what the track holds. A
     curve's bar has its least and its most beside its two ends, each a number that a click makes
     a field. Where the lane's axis runs up, the bar's top is the highest. -->
<script lang="ts">
	import type { Track } from '$lib/api/generated';
	import RenameField from '$lib/inspector/RenameField.svelte';
	import { fixed } from '$lib/ui';
	import RangeBar from './RangeBar.svelte';

	let {
		t,
		up,
		onPreview,
		onCommit
	}: {
		t: Track;
		/** The lane's own axis runs up, so the bar stands and its top is the highest; else the bar
		 * lies and its end is. */
		up: boolean;
		/** The range as a drag has it. */
		onPreview: (range: [number, number]) => void;
		/** The range to send as one edit. */
		onCommit: (range: [number, number]) => void;
	} = $props();

	/** Every key there is: a keyboard's range is some of them. */
	const KEYS = 128;
	/** The stretch of a curve's values the bar is as long as: from the least value a keyframe has
	 * to the most, and the track's own range where the keyframes have fewer than two values. */
	const extent = $derived.by<[number, number]>(() => {
		const values = t.kind === 'curve' ? t.keys.map((k) => k.value) : [];
		const [least, most] = [Math.min(...values), Math.max(...values)];
		return most > least ? [least, most] : [t.range[0], t.range[1]];
	});
	/** The stretch a drag began on: the bar is as long as that until the drag ends, whatever the
	 * range has become meanwhile. */
	let held = $state<[number, number] | null>(null);
	/** The bar's axis, from 0: where it begins on the track's own scale, and how long it is. A
	 * keyboard's last key takes room too, so its range ends one past it. */
	const [origin, span] = $derived(t.kind === 'note' ? [0, KEYS] : [(held ?? extent)[0], (held ?? extent)[1] - (held ?? extent)[0]]);
	const past = $derived(t.kind === 'note' ? 1 : 0);
	/** The range on the bar's axis, and a view of the bar as a range. A bar that stands is
	 * measured from its top, where the highest is. */
	const view = $derived<[number, number]>(up ? [span - (t.range[1] + past - origin), span - (t.range[0] - origin)] : [t.range[0] - origin, t.range[1] + past - origin]);
	const ranged = (v: [number, number]): [number, number] => {
		const [low, high] = up ? [span - v[1], span - v[0]] : v;
		return [fixed(low + origin), fixed(high - past + origin)];
	};
	/** The range that holds all the track has: its keyframes' values, or its notes' keys and no
	 * less than an octave of them. None where the track has nothing to fit. */
	const whole = $derived.by<[number, number] | null>(() => {
		if (t.kind === 'curve') return extent[0] === t.range[0] && extent[1] === t.range[1] ? null : extent;
		if (t.notes.length === 0) return null;
		const low = Math.min(...t.notes.map((n) => n.pitch));
		return [Math.min(low, KEYS - 13), Math.max(...t.notes.map((n) => n.pitch), Math.min(low, KEYS - 13) + 12)];
	});
	/** A limit typed at an end of a curve's bar. */
	const limited = (end: 0 | 1, text: string): void => onCommit(t.range.with(end, Number(text)) as [number, number]);
	const apart = (end: 0 | 1) => (text: string): boolean => text !== '' && Number.isFinite(Number(text)) && (end === 0 ? Number(text) < t.range[1] : Number(text) > t.range[0]);
</script>

{#snippet limit(which: 0 | 1)}
	<span class="limit" class:most={which === 1} title={which ? 'The most the lane shows: click to set it' : 'The least the lane shows: click to set it'}>
		<RenameField value={String(t.range[which])} valid={apart(which)} onRename={(text) => limited(which, text)} label={which ? 'Most' : 'Least'} testid={which ? 'track-range-most' : 'track-range-least'} />
	</span>
{/snippet}

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="range" class:up onpointerdowncapture={() => (held = extent)} onpointerup={() => (held = null)} onpointercancel={() => (held = null)}>
	<RangeBar
		{view}
		{span}
		vertical={up}
		bounded={t.kind === 'note'}
		step={t.kind === 'note' ? 1 : 0}
		least={t.kind === 'note' ? 2 : span / 1000}
		testid="track-range"
		onChange={(v) => onPreview(ranged(v))}
		onCommit={(v) => onCommit(ranged(v))}
		onReset={() => whole && onCommit(whole)}
	/>
	{#if t.kind === 'curve'}
		{@render limit(1)}
		{@render limit(0)}
	{/if}
</div>

<style>
	/* The bar, as long as the room it has along the lane's own axis: it stands where that runs
	   up, and lies where it runs to the right. */
	.range {
		position: relative;
		display: flex;
		flex-direction: column;
		width: 100%;
		height: 100%;
	}
	.range.up {
		flex-direction: row;
	}
	/* A limit is a number in small type beside its end of the bar, in the room `--keys` wide
	   that a keyboard has there: the most at the bar's top or its right end, the least at the
	   other. The field it becomes is wide enough to type in, and lies over the lane. */
	.limit {
		position: absolute;
		top: 100%;
		left: 0;
		display: flex;
		align-items: flex-start;
		max-width: 50%;
		font-size: var(--fs-micro);
	}
	.limit.most {
		left: auto;
		right: 0;
	}
	.up .limit {
		top: auto;
		bottom: 0;
		left: 100%;
		right: auto;
		align-items: center;
		width: var(--keys, calc(var(--space-8) * 1.5));
		max-width: none;
		padding-left: var(--space-1);
	}
	.up .limit.most {
		top: 0;
		bottom: auto;
	}
	.limit :global(.rename) {
		position: relative;
		z-index: 3;
		flex: none;
		width: calc(var(--space-8) * 3);
	}
	/* Under a finger a limit is a press as large as any control's. */
	@media (hover: none) and (pointer: coarse) {
		.limit :global(.name) {
			min-width: var(--hit);
			min-height: var(--hit);
		}
	}
</style>
