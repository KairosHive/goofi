<!-- Timeline panel: every track of the patch stacked on one time axis — a note track as a piano
     roll, a curve track as one lane per attribute — under the clock they play on. The tracks are
     the patch's; the tracks a panel hides and the unit of its ruler are its own state. Every change
     is a `timeline` or a `clock` op, so the manager owns the model and this panel the drawing and
     the gesture. -->
<script lang="ts">
	import { ContextMenu, MOVE_TOLERANCE_PX, asStateObject, beginDrag, createLongPress, type MenuItem, type PanelProps } from 'panelty';
	import type { Key, Note, Track } from '$lib/api/generated';
	import { isValidIdentifier } from '$lib/crdt/graphDoc';
	import { graph } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import { contentEnd, hiding, lanePath, pitches, ticks, unit } from '$lib/timeline/model';
	import { Bar, Button, ConfirmDialog, CurvePicker, EmptyState, Icon, IconButton, NumberInput, ScrollArea, Segmented, TextInput } from '$lib/ui';

	interface TimelineState {
		/** The ids of the tracks this panel hides. */
		hidden?: string[];
		/** Whether the ruler counts beats and not seconds. */
		beats?: boolean;
	}
	const UNITS = [
		{ id: 's', label: 's', title: 'Times in seconds' },
		{ id: 'beats', label: 'beats', title: 'Times in beats of the tempo' }
	];

	let { state: panelState, setState }: PanelProps = $props();
	const g = graph();
	const view = $derived(asStateObject(panelState) as TimelineState);
	const hidden = $derived(view.hidden ?? []);
	const shown = $derived(Object.keys(g.timeline).filter((id) => !hidden.includes(id)));
	// An id the timeline does not hold is a removed track's: the panel passes over it.
	const stowed = $derived(hidden.filter((id) => id in g.timeline));
	const fail = (what: string) => (e: unknown) => notify().failure(what, e);

	// ---- the clock: its variables read live, `clock set` to steer it.
	watchVariables(() => ['time', 'playing', 'bpm', 'length', 'loop'].map((n) => `clock.${n}`));
	function clock(name: string): number {
		const v = variableValue(`clock.${name}`);
		return typeof v === 'number' ? v : 0;
	}
	const playing = $derived(clock('playing') > 0);
	/** The seconds one mark of the ruler counts. */
	const mark = $derived(unit(view.beats === true, clock('bpm')));
	/** What a drag holds still, so its own previews move neither: the axis, and its lane's range. */
	let frozen = $state<{ span: number; track: string; lane: string; index: number; range: [number, number] } | null>(null);
	/** The seconds the axis spans: the clock's length while it restarts there, else what the tracks hold. */
	const span = $derived(frozen?.span ?? (clock('loop') > 0 && clock('length') > 0 ? clock('length') : Math.max(8, contentEnd(Object.values(g.timeline), clock('bpm')) * 1.25)));
	const steer = (to: { playing?: boolean; time?: number }): void => void g.op('clock set', to).catch(fail('Set clock'));
	const set = (name: string, value: number): void => void g.setVariableValue(`clock.${name}`, value).catch(fail('Set clock'));
	const fixed = (v: number): number => Number(v.toFixed(3));
	const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));
	const fraction = (p: { clientX: number; clientY: number }, el: Element): [number, number] => {
		const box = el.getBoundingClientRect();
		return [(p.clientX - box.left) / box.width, (p.clientY - box.top) / box.height];
	};

	// ---- the tracks: an edit is one op on the track; what a panel hides is the panel's state.
	const edit = (track: string, patch: Record<string, unknown>): void => void g.op('timeline track edit', { track, ...patch }).catch(fail('Edit track'));
	const add = (kind: Track['kind']): void => void g.op('timeline track add', { kind, beats: view.beats === true }).catch(fail('Add track'));
	const hide = (ids: string[], label: string): void => setState({ hidden: ids }, 'authored', label);
	const rename = (id: string, to: string): void => {
		if (to !== g.timeline[id].name && isValidIdentifier(to)) edit(id, { to });
	};
	/** The track asked to be deleted, until the dialog answers or the track goes. */
	let asked = $state('');
	const doomed = $derived(g.timeline[asked]);
	$effect(() => {
		if (!doomed) asked = '';
	});

	let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
	function trackMenu(id: string, x: number, y: number): void {
		const entry = (which: Parameters<typeof hiding>[2], label: string): MenuItem => {
			const more = hiding(shown, id, which);
			return { label, disabled: more.length === 0, action: () => hide([...hidden, ...more], label) };
		};
		const items = [entry('this', 'Hide Track'), entry('above', 'Hide Tracks Above'), entry('below', 'Hide Tracks Below'), entry('others', 'Hide Other Tracks')];
		menu = { x, y, items: [...items, { separator: true }, { label: 'Delete Track…', icon: 'x', action: () => (asked = id) }] };
	}
	/** A menu that drops from the bar button that opened it. */
	function drop(e: MouseEvent, items: MenuItem[]): void {
		const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
		menu = { x: box.left, y: box.bottom, items };
	}
	// The touch door onto a track's menu.
	let pressed = '';
	const press = createLongPress((at) => trackMenu(pressed, at.clientX, at.clientY));

	// ---- the notes and the keyframes: what a lane holds, a note track's lane being the empty name.
	let picked = $state<{ track: string; lane: string; index: number } | null>(null);
	/** Pick a note or a keyframe, or nothing. The focus leaves first, so a field that shows the one
	 * picked before commits to that one. */
	function pick(next: typeof picked): void {
		(document.activeElement as HTMLElement | null)?.blur();
		picked = next;
	}
	const held = (t: Track | undefined, lane: string): (Note | Key)[] => (t?.kind === 'note' ? t.notes : (t?.attributes[lane]?.keys ?? []));
	const put = (t: Track, lane: string, items: unknown[]): Record<string, unknown> =>
		t.kind === 'note' ? { notes: items } : { attributes: { [lane]: { ...t.attributes[lane], keys: items } } };
	const range = (t: Track, lane: string): [number, number] => (t.kind === 'note' ? pitches(t.notes) : t.attributes[lane].range);
	const units = (t: Track): number => span / unit(t.beats, clock('bpm'));
	/** Change the picked note or keyframe, or with null remove it. */
	function change(id: string, t: Track, to: Partial<Note & Key> | null): void {
		const { lane, index } = picked!;
		if (!to) picked = null;
		edit(id, put(t, lane, held(t, lane).flatMap((o, k) => (k !== index ? [o] : to ? [{ ...o, ...to }] : []))));
	}
	/** A double click on a lane's own ground adds a note or a keyframe there. */
	function addAt(e: MouseEvent, id: string, t: Track, lane: string): void {
		if (e.target !== e.currentTarget) return;
		const [fx, fy] = fraction(e, e.currentTarget as Element);
		const [lo, hi] = range(t, lane);
		const at = fixed(fx * units(t));
		const born = t.kind === 'note' ? { at, length: t.beats ? 1 : 0.5, pitch: clamp(hi - Math.floor(fy * (hi - lo + 1)), lo, hi), velocity: 0.8 } : { at, value: fixed(clamp(hi - fy * (hi - lo), lo, hi)), curve: 'linear' };
		edit(id, put(t, lane, [...held(t, lane), born]));
	}
	/** A drag of one note or keyframe by as much as the pointer moves: each move previews the lane
	 * as it stands with that one moved, and the release sends the last of them as the edit. A press
	 * that does not move is a pick alone. */
	let drag: (() => void) | undefined;
	function dragItem(e: PointerEvent, id: string, t: Track, lane: string, index: number): void {
		e.stopPropagation();
		pick({ track: id, lane, index });
		const ground = (e.currentTarget as HTMLElement).closest('.ground')!;
		const [x0, y0] = fraction(e, ground);
		const [lo, hi] = range(t, lane);
		const [o, per] = [held(t, lane)[index], units(t)];
		const now = (): (Note | Key)[] => held(g.timeline[id], lane);
		/** The lane as it stands, with the dragged one where `to` puts it. */
		const moved = (to: Partial<Note & Key>): Record<string, unknown> => put(g.timeline[id] ?? t, lane, now().map((it, k) => (k === index ? { ...it, ...to } : it)));
		let patch: Record<string, unknown> | null = null;
		drag = beginDrag(e.currentTarget as HTMLElement, e.pointerId, {
			move: (p) => {
				if (!patch && Math.hypot(p.clientX - e.clientX, p.clientY - e.clientY) < MOVE_TOLERANCE_PX) return;
				frozen ??= { span, track: id, lane, index, range: [lo, hi] };
				const [fx, fy] = fraction(p, ground);
				const at = o.at + (fx - x0) * per;
				// A keyframe stays between its neighbours and inside the attribute's range.
				patch = moved(
					'pitch' in o
						? { at: fixed(Math.max(0, at)), pitch: clamp(o.pitch - Math.round((fy - y0) * (hi - lo + 1)), lo, hi) }
						: { at: fixed(clamp(at, Math.max(0, (now()[index - 1]?.at ?? -1) + 0.001), (now()[index + 1]?.at ?? Infinity) - 0.001)), value: fixed(clamp(o.value - (fy - y0) * (hi - lo), lo, hi)) }
				);
				g.previewTrack(id, patch);
			},
			commit: () => {
				frozen = null;
				if (patch) edit(id, patch);
			},
			// Put back, unless the dragged one left the lane under the drag.
			cancel: () => {
				frozen = null;
				if (patch && now()[index]) g.previewTrack(id, moved('pitch' in o ? { at: o.at, pitch: o.pitch } : { at: o.at, value: o.value }));
			}
		});
	}
	// A drag ends with the panel, and with the note or keyframe it holds.
	$effect(() => {
		if (frozen && !held(g.timeline[frozen.track], frozen.lane)[frozen.index]) drag?.();
	});
	$effect(() => () => {
		press.cancel();
		drag?.();
	});
	/** A new attribute on a curve track, under the first free name. */
	function addAttribute(id: string, t: Extract<Track, { kind: 'curve' }>): void {
		const names = Object.values(t.attributes).map((l) => l.name);
		const name = Array.from({ length: names.length + 1 }, (_, n) => `attr${n}`).find((n) => !names.includes(n))!;
		edit(id, { attributes: { [name]: { name, range: [0, 1], keys: [] } } });
	}
</script>

<div class="timeline" data-testid="timeline-panel">
	<Bar style="--bar-wrap: wrap">
		{#snippet start()}
			<IconButton label={playing ? 'Pause' : 'Play'} data-testid="clock-play" onclick={() => steer({ playing: !playing })}><Icon name={playing ? 'pause' : 'play'} /></IconButton>
			<IconButton label="Stop" data-testid="clock-stop" onclick={() => steer({ playing: false, time: 0 })}><Icon name="square" /></IconButton>
			<NumberInput aria-label="Position" title="The clock's position" data-testid="clock-time" value={Number((clock('time') / mark).toFixed(2))} min={0} onChange={(v) => steer({ time: v * mark })} />
			<Segmented aria-label="Ruler unit" value={view.beats ? 'beats' : 's'} segments={UNITS} data-testid="timeline-unit" onChange={(u) => setState({ beats: u === 'beats' }, 'authored', 'Set ruler unit')} />
		{/snippet}
		{#snippet end()}
			<NumberInput aria-label="Tempo" title="The tempo, in beats a minute" data-testid="clock-bpm" value={clock('bpm')} min={0} max={999} onChange={(v) => set('bpm', v)} />
			<Segmented value={clock('loop') > 0 ? 'loop' : null} segments={[{ id: 'loop', label: 'loop', title: 'Restart the clock at the length', testid: 'clock-loop' }]} onChange={() => set('loop', clock('loop') > 0 ? 0 : 1)} />
			<NumberInput aria-label="Length" title="Where the clock restarts while it loops" data-testid="clock-length" value={Number((clock('length') / mark).toFixed(2))} min={0} onChange={(v) => set('length', v * mark)} />
			{#if stowed.length > 0}
				<Button size="sm" data-testid="timeline-hidden" title="Hidden tracks: pick one to show it" onclick={(e) => drop(e, stowed.map((id) => ({ label: g.timeline[id].name, action: () => hide(hidden.filter((h) => h !== id), 'Show Track') })))}>
					{stowed.length} hidden <Icon name="chevron-down" />
				</Button>
			{/if}
			<IconButton label="Add track" data-testid="timeline-add" onclick={(e) => drop(e, [{ label: 'Note Track', action: () => add('note') }, { label: 'Curve Track', action: () => add('curve') }])}><Icon name="plus" /></IconButton>
		{/snippet}
	</Bar>
	<ScrollArea>
		<div class="stack">
			<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
			<div class="row ruler" data-testid="timeline-ruler">
				<span></span>
				<div class="lane" onclick={(e) => steer({ time: fraction(e, e.currentTarget)[0] * span })} title="Click to put the clock's position here">
					{#each ticks(span / mark) as tick (tick)}<span class="tick" style:left={`${(100 * tick * mark) / span}%`}>{tick}</span>{/each}
				</div>
			</div>
			{#each shown as id (id)}
				{@const t = g.timeline[id]}
				{@const mine = picked?.track === id ? held(t, picked.lane)[picked.index] : undefined}
				<!-- svelte-ignore a11y_no_static_element_interactions -->
				<section
					class="track"
					data-testid={`track-${t.name}`}
					oncontextmenu={(e) => {
						e.preventDefault();
						trackMenu(id, e.clientX, e.clientY);
					}}
					onpointerdown={(e) => {
						pressed = id;
						if (e.pointerType !== 'mouse') press.start(e);
					}}
					onpointermove={press.move}
					onpointerup={press.cancel}
					onpointercancel={press.cancel}
				>
					<header>
						<TextInput class="name" aria-label="Track name" value={t.name} onChange={(to) => rename(id, to)} />
						<Segmented aria-label="Time unit" value={t.beats ? 'beats' : 's'} segments={UNITS} onChange={(u) => edit(id, { beats: u === 'beats' })} />
						{#if t.kind === 'curve'}
							<IconButton size="sm" label="Add attribute" data-testid="track-add-attribute" onclick={() => addAttribute(id, t)}><Icon name="plus" /></IconButton>
						{/if}
						<span class="grow"></span>
						{#if mine && picked}
							{#if 'pitch' in mine}
								<NumberInput aria-label="Length" title="How long the note lasts" value={mine.length} min={0.01} step={0.25} onChange={(length) => change(id, t, { length })} />
								<NumberInput aria-label="Velocity" title="The note's velocity, 0 to 1" value={mine.velocity} min={0} max={1} step={0.05} onChange={(velocity) => change(id, t, { velocity })} />
							{:else}
								{@const [min, max] = range(t, picked.lane)}
								<NumberInput aria-label="Value" title="The keyframe's value" value={mine.value} {min} {max} step={0.01} onChange={(value) => change(id, t, { value })} />
								<CurvePicker value={mine.curve} onChange={(curve) => change(id, t, { curve })} />
							{/if}
							<IconButton size="sm" label={'pitch' in mine ? 'Delete note' : 'Delete keyframe'} data-testid="track-delete-picked" onclick={() => change(id, t, null)}><Icon name="x" /></IconButton>
						{/if}
						<IconButton size="sm" label="Hide track" data-testid="track-hide" onclick={() => hide([...hidden, id], 'Hide Track')}><Icon name="eye-off" /></IconButton>
					</header>
					{#if t.kind === 'note'}
						{@const [lo, hi] = frozen?.track === id ? frozen.range : pitches(t.notes)}
						<div class="row">
							<span class="side">{t.notes.length} notes</span>
							<div class="lane">
								<!-- svelte-ignore a11y_no_static_element_interactions -->
								<div class="ground" style:--rows={hi - lo + 1} data-testid="track-lane" ondblclick={(e) => addAt(e, id, t, '')} onpointerdown={() => pick(null)}>
									{#each t.notes as n, i}
										<div
											class="note"
											class:picked={mine === n}
											style:left={`${(100 * n.at) / units(t)}%`}
											style:width={`${(100 * n.length) / units(t)}%`}
											style:top={`${(100 * (hi - n.pitch)) / (hi - lo + 1)}%`}
											style:height={`${100 / (hi - lo + 1)}%`}
											style:opacity={0.4 + 0.6 * n.velocity}
											onpointerdown={(e) => dragItem(e, id, t, '', i)}
										></div>
									{/each}
								</div>
							</div>
						</div>
					{:else}
						{#each Object.entries(t.attributes) as [aid, lane] (aid)}
							<div class="row">
								<span class="side">
									<TextInput class="name" aria-label="Attribute name" value={lane.name} onChange={(name) => isValidIdentifier(name) && edit(id, { attributes: { [aid]: { ...lane, name } } })} />
									{#each [0, 1] as end (end)}
										<NumberInput aria-label={end ? 'Maximum' : 'Minimum'} title={end ? 'The highest value' : 'The lowest value'} value={lane.range[end]} step={0.1} onChange={(v) => edit(id, { attributes: { [aid]: { ...lane, range: lane.range.with(end, v) } } })} />
									{/each}
									<IconButton size="sm" label="Remove attribute" onclick={() => edit(id, { attributes: { [aid]: null } })}><Icon name="x" /></IconButton>
								</span>
								<div class="lane">
									<!-- svelte-ignore a11y_no_static_element_interactions -->
									<div class="ground" data-testid={`track-lane-${lane.name}`} ondblclick={(e) => addAt(e, id, t, aid)} onpointerdown={() => pick(null)}>
										<svg viewBox="0 0 1000 100" preserveAspectRatio="none" aria-hidden="true"><path d={lanePath(lane, units(t), 1000, 100)} vector-effect="non-scaling-stroke" /></svg>
										{#each lane.keys as k, i}
											<span
												class="key"
												class:picked={mine === k}
												style:left={`${(100 * k.at) / units(t)}%`}
												style:top={`${(100 * (lane.range[1] - k.value)) / (lane.range[1] - lane.range[0] || 1)}%`}
												onpointerdown={(e) => dragItem(e, id, t, aid, i)}
											></span>
										{/each}
									</div>
								</div>
							</div>
						{/each}
					{/if}
				</section>
			{/each}
			{#if clock('time') <= span}
				<div class="playhead" data-testid="timeline-playhead" style:--at={clock('time') / span}></div>
			{/if}
		</div>
		{#if shown.length === 0}
			<EmptyState>
				{#snippet title()}{stowed.length > 0 ? 'Every track is hidden' : 'No tracks'}{/snippet}
				{#snippet hint()}{stowed.length > 0 ? 'Show one from the hidden tracks in the bar.' : 'Add a note track or a curve track with + in the bar.'}{/snippet}
			</EmptyState>
		{/if}
	</ScrollArea>
</div>

{#if menu}
	<ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => (menu = null)} />
{/if}
<ConfirmDialog
	open={doomed !== undefined}
	question={`Delete track ${doomed?.name}?`}
	detail="Its notes or keyframes and its variable group go with it. Undo brings the track back."
	onClose={() => (asked = '')}
	data-testid="track-delete-dialog"
>
	<Button
		variant="danger"
		data-testid="track-delete-confirm"
		onclick={() => {
			void g.op('timeline track remove', { track: asked }).catch(fail('Delete track'));
			asked = '';
		}}>Delete</Button
	>
	<Button variant="ghost" onclick={() => (asked = '')}>Cancel</Button>
</ConfirmDialog>

<style>
	.timeline {
		/* The column beside the lanes: what a row says about itself. A pitch's row in a note lane. */
		--side: calc(var(--space-8) * 6);
		--row: var(--space-3);
		--number-width: calc(var(--space-8) * 2.5);
		display: flex;
		flex-direction: column;
		height: 100%;
		min-height: 0;
	}
	.stack {
		position: relative;
	}
	.track {
		border-top: 1px solid var(--border);
	}
	header,
	.side {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: var(--space-2);
		min-width: 0;
		padding: var(--space-1) var(--space-2);
		font-size: var(--fs-small);
		color: var(--text-muted);
	}
	/* A row's side: the name on a line of its own, the range and the ✕ under it. */
	.side {
		--number-width: calc(var(--space-8) * 1.75);
	}
	.side :global(.name) {
		flex: 1 1 100%;
	}
	header {
		flex-wrap: nowrap;
	}
	header :global(.name) {
		flex: 0 1 calc(var(--side) - var(--space-4));
		min-width: 0;
	}
	.grow {
		flex: 1 1 0;
	}
	.row {
		display: grid;
		grid-template-columns: var(--side) minmax(0, 1fr);
	}
	/* The room above and under the ground shows a keyframe at the end of its range whole. */
	.lane {
		position: relative;
		padding: var(--space-3) 0;
		background: var(--surface-2);
		border-bottom: 1px solid var(--bg);
		overflow: hidden;
		touch-action: none;
		user-select: none;
	}
	.ground {
		position: relative;
		height: calc(var(--rows, 10) * var(--row));
	}
	.ruler {
		position: sticky;
		top: 0;
		z-index: 1;
		background: var(--surface-1);
	}
	.ruler .lane {
		height: var(--space-8);
		padding: 0;
		cursor: pointer;
	}
	.tick {
		position: absolute;
		top: 0;
		bottom: 0;
		padding-left: var(--space-1);
		border-left: 1px solid var(--border-strong);
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		color: var(--text-muted);
		pointer-events: none;
	}
	.note {
		position: absolute;
		min-width: var(--space-2);
		border-radius: var(--radius-sm);
		background: var(--accent);
		cursor: grab;
	}
	svg {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		overflow: visible;
		pointer-events: none;
	}
	path {
		fill: none;
		stroke: var(--accent);
		stroke-width: 1.5;
	}
	.key {
		position: absolute;
		width: var(--space-5);
		height: var(--space-5);
		translate: -50% -50%;
		border-radius: 50%;
		background: var(--accent-strong);
		cursor: grab;
	}
	.picked {
		outline: var(--focus-width) solid var(--focus-ink);
	}
	.playhead {
		position: absolute;
		top: 0;
		bottom: 0;
		left: calc(var(--side) + (100% - var(--side)) * var(--at));
		width: 1px;
		background: var(--danger);
		pointer-events: none;
	}
	/* Under a finger a row is taller, and a keyframe takes a press a hit-size wide, behind every
	   keyframe's own dot. */
	@media (hover: none) and (pointer: coarse) {
		.timeline {
			--row: var(--space-5);
		}
		.ground {
			isolation: isolate;
		}
		.key::after {
			content: '';
			position: absolute;
			inset: calc((100% - var(--hit)) / 2);
			z-index: -1;
		}
	}
</style>
