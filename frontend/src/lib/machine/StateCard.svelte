<!-- A state's box on the machine canvas: its name and its menu of attributes and playheads, then
     how it decides — the mode, the interval knob, and a line per transition out, linked shares in
     `random`. Its border band is the one connection handle, so a drag from the edge draws a
     transition and a drag from the name moves the box; the controls take their own gestures. -->
<script lang="ts">
	import { Handle, Position, type NodeProps } from '@xyflow/svelte';
	import { ContextMenu, createLongPress, type MenuItem } from 'panelty';
	import type { Decision } from '$lib/api/generated';
	import ExprEditor from '$lib/inspector/expr/ExprEditor.svelte';
	import { graph } from '$lib/stores/graph.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { Knob, Segmented, Slider } from '$lib/ui';
	import { DECISIONS, outgoing, relink, shares, weightsOf } from './decision';
	import { CARD_HEAD, CARD_LINE, CARD_W } from './layout';
	import StateMenu from './StateMenu.svelte';

	let { id, data }: NodeProps = $props();
	const g = graph();
	const machine = $derived(data.machine as string);
	const m = $derived(g.machines[machine]);
	const s = $derived(m?.states[id]);
	const out = $derived(m ? outgoing(m, id) : []);
	const target = (t: string): string => m.transitions[t].to;

	function edit(patch: Record<string, unknown>): void {
		void g.machine('machine state edit', { machine, name: id, ...patch }).catch((e) => notify().failure('Edit state', e));
	}

	// ---- the draw's shares: a drag moves one, the rest follow in their own ratio, and the release
	// lands all of them as one edit. The drag reads from the shares it started on, so nothing drifts.
	let draft = $state<{ base: number[]; now: number[] } | null>(null);
	const committed = $derived(s ? shares(s, out) : [1]);
	const shown = $derived(draft?.now ?? committed);
	function drag(k: number, v: number): void {
		const base = draft?.base ?? committed;
		draft = { base, now: relink(base, k, v) };
		g.previewState(machine, id, weightsOf(draft.now, out));
	}
	function land(k: number, v: number): void {
		const p = relink(draft?.base ?? committed, k, v);
		draft = null;
		edit(weightsOf(p, out));
	}
	/** A control's keys are its own: Flow would move a selected card on an arrow and delete it on Backspace. */
	const keep = (e: KeyboardEvent): void => e.stopPropagation();
	const percent = (p: number): string => `${Math.round(p * 100)}%`;

	let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);
	function openMenu(x: number, y: number): void {
		menu = {
			x,
			y,
			items: [
				{
					label: 'Delete state',
					icon: 'x',
					action: () => void g.machine('machine state remove', { machine, name: id }).catch((e) => notify().failure('Remove state', e))
				}
			]
		};
	}
	// The touch door onto the menu; the click its release fires must not then select the box.
	const press = createLongPress((at) => openMenu(at.clientX, at.clientY));
	const SEGMENTS = DECISIONS.map((d) => ({ id: d.id, label: d.label, title: d.title, testid: `state-mode-${d.id}` }));
</script>

<div class="card" style={`width: ${CARD_W}px; --head: ${CARD_HEAD}px; --line: ${CARD_LINE}px`} data-testid={`state-card-${id}`} data-state={id} data-mode={s?.mode}>
	<Handle type="source" position={Position.Top} class="port" />
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div
		class="body"
		oncontextmenu={(e) => {
			e.preventDefault();
			openMenu(e.clientX, e.clientY);
		}}
		onpointerdown={(e) => {
			if (e.pointerType !== 'mouse') press.start(e);
		}}
		onpointermove={press.move}
		onpointerup={press.cancel}
		onpointercancel={press.cancel}
	>
		<div class="title">
			<span class="name" data-testid="state-card-name">{id}</span>
			{#if m}<span class="nodrag nopan" onkeydown={keep}><StateMenu name={machine} {m} {id} /></span>{/if}
		</div>
		{#if s}
			<div class="controls nodrag nopan" onkeydown={keep}>
				<Segmented value={s.mode} segments={SEGMENTS} onChange={(mode) => edit({ mode: mode as Decision })} aria-label="decision" data-testid="state-mode" fill />
				<span class="knob" title="Seconds between decisions; 0 decides once, on arrival">
					<Knob value={s.interval} min={0} max={10} step={0.05} label="interval" data-testid="state-interval"
						onInput={(v) => g.previewState(machine, id, { interval: v })} onChange={(v) => edit({ interval: v })} />
				</span>
				<span class="secs" data-testid="state-interval-value">{s.interval.toFixed(2)}s</span>
			</div>
			<div class="lines nodrag nopan" onkeydown={keep}>
				{#if s.mode === 'random'}
					<div class="line" data-testid="state-share-wait">
						<span class="to muted">wait</span>
						<Slider value={shown[0]} min={0} max={1} step={0.01} bounds={false} aria-label="wait" onInput={(v) => drag(0, v)} onChange={(v) => land(0, v)} />
						<span class="pct">{percent(shown[0])}</span>
					</div>
				{:else if s.mode === 'expression'}
					<div class="line expr">
						<ExprEditor value={s.expression} onCommit={(e) => edit({ expression: e })} label="decision" placeholder="index, e.g. int(variables.desk.pick)" testid="state-expression" />
					</div>
				{:else}
					<div class="line"><span class="to muted">in turn, each interval</span></div>
				{/if}
				{#each out as t, k (t)}
					<div class="line" data-testid={`state-share-${t}`} title={t}>
						{#if s.mode === 'random'}
							<span class="to">→ {target(t)}</span>
							<Slider value={shown[k + 1]} min={0} max={1} step={0.01} bounds={false} aria-label={`share of ${t}`} onInput={(v) => drag(k + 1, v)} onChange={(v) => land(k + 1, v)} />
							<span class="pct">{percent(shown[k + 1])}</span>
						{:else}
							<span class="index">{s.mode === 'sequence' ? k + 1 : k}</span>
							<span class="to">→ {target(t)}</span>
						{/if}
					</div>
				{/each}
			</div>
		{/if}
	</div>
</div>

{#if menu}
	<ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => (menu = null)} />
{/if}

<style>
	.card {
		position: relative;
		box-sizing: border-box;
		background: var(--surface-1);
		border: 1px solid var(--border-strong);
		border-radius: var(--radius-md);
		font-size: var(--fs-small);
	}
	:global(.svelte-flow__node.selected) .card {
		outline: var(--focus-width) solid var(--accent);
	}
	/* The handle IS the card, under the body: what shows of it is the band around the body. */
	.card :global(.port) {
		position: absolute;
		inset: 0;
		width: auto;
		height: auto;
		min-width: 0;
		min-height: 0;
		transform: none;
		border: none;
		border-radius: var(--radius-md);
		background: transparent;
		cursor: crosshair;
	}
	/* The outline lights and thickens fourfold under the pointer — inward, as a shadow, so nothing
	   moves — and while a cable in flight has this box as its target. */
	.card:hover,
	.card:has(> :global(.port.connectingto)),
	.card:has(> :global(.port.valid)) {
		border-color: var(--accent);
		box-shadow: inset 0 0 0 3px var(--accent);
	}
	/* Under the pointer the band reaches half again as far, outward, so the box keeps its size. */
	.card:hover :global(.port) {
		inset: calc(-1 * var(--space-3));
	}
	/* The rows are fixed heights, so the card's footprint is the one `cardHeight` and the manager's
	   placement count: the band, the title, the controls, then the lines. */
	.body {
		position: relative;
		z-index: 1;
		display: flex;
		flex-direction: column;
		margin: var(--space-5);
		border-radius: var(--radius-sm);
		background: var(--surface-1);
	}
	.title {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		height: 34px;
		padding: 0 var(--space-2);
	}
	.name {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: var(--font-mono);
		font-size: var(--fs-display);
		font-weight: 600;
	}
	.controls {
		display: flex;
		align-items: center;
		gap: var(--space-3);
		/* What the head leaves of the card's band, border and title; a finger's knob may grow it. */
		height: max(var(--hit), calc(var(--head) - 2 * var(--space-5) - 2px - 34px));
		cursor: default;
	}
	.knob {
		display: inline-flex;
		width: var(--hit);
		height: var(--hit);
	}
	.secs,
	.pct,
	.index {
		flex: 0 0 auto;
		font-family: var(--font-mono);
		font-variant-numeric: tabular-nums;
		color: var(--text-muted);
	}
	.secs {
		width: 3.2em;
		text-align: right;
	}
	.pct {
		width: 2.8em;
		text-align: right;
	}
	.lines {
		display: flex;
		flex-direction: column;
		cursor: default;
	}
	.line {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		height: var(--line);
		min-width: 0;
	}
	.line.expr > :global(*) {
		flex: 1 1 auto;
		min-width: 0;
	}
	.to {
		flex: 0 1 auto;
		width: 5.5em;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: var(--font-mono);
	}
	.muted {
		color: var(--text-muted);
	}
	/* A finger needs a wider band to start a transition from. */
	@media (hover: none) and (pointer: coarse) {
		.body {
			margin: var(--space-7);
		}
	}
</style>
