<!-- The score: the machine's states as columns, in loop order and as wide as their intervals, its
     attributes as rows, and every cell a vertical slider — so a melody is dragged across the
     columns, and read off them. The lane above the columns draws the transitions as arcs. A
     column's cog opens the state's settings, a row's the attribute's, a cell's the value with the
     classic widget; a drag on a column's edge sets its interval. The canvas's selection lights its
     columns, a paint in one of them lands in all, and the pointer on either lights the other. -->
<script lang="ts">
	import type { Attribute, Literal, Machine } from '$lib/api/generated';
	import { beginDrag } from 'panelty';
	import { isValidIdentifier } from '$lib/crdt/graphDoc';
	import ParamField from '$lib/inspector/ParamField.svelte';
	import { graph } from '$lib/stores/graph.svelte';
	import { history } from '$lib/stores/history.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { selection } from '$lib/stores/selection.svelte';
	import { variableFrame, variableValue, watchVariables } from '$lib/stores/variableValues.svelte';
	import { Button, ENVELOPE_BORN, EmptyState, EnvelopePicker, Field, Icon, IconButton, Popover, TextInput, Toggle, colorCss, curvePath } from '$lib/ui';
	import { envelopePoints, pathOf } from '$lib/ui/envelope';
	import AttributeForm from './AttributeForm.svelte';
	import { FACES, brief, descriptorFor, envelopeDescriptor, faceOf, fractionOf, literalOf, valueAt } from './attributes';
	import { machineHover } from './hover.svelte';
	import { SCORE, dotColor } from './layout';
	import { editEach, type Subject } from './model';
	import { columnOrder, duration, joined, widths } from './order';
	import StateForm from './StateForm.svelte';
	import TransitionForm from './TransitionForm.svelte';
	import TransitionLane from './TransitionLane.svelte';

	let { name, m, subject, panelId, onClose }: { name: string; m: Machine; subject: Subject; panelId: string; onClose: () => void } = $props();

	const g = graph();
	const sel = selection();
	const hover = machineHover();
	const modifier = (e: MouseEvent): boolean => e.shiftKey || e.ctrlKey || e.metaKey || sel.multiSelect;

	// ---- the columns: the states in loop order, each as wide as its interval, filling the score.
	const order = $derived(columnOrder(m));
	const attrs = $derived(Object.keys(m.attributes));
	const selected = $derived(subject.selected.filter((s) => s in m.states));
	const picked = $derived(new Set(subject.open));
	let scoreW = $state(0);
	const cols = $derived(widths(order.map((s) => duration(m, s)), Math.max(0, scoreW - SCORE.gutter - SCORE.add), SCORE.minColumn));
	const lefts = $derived(cols.reduce<number[]>((acc, w, i) => [...acc, (acc[i - 1] ?? 0) + (cols[i - 1] ?? 0)], []).map((x) => x));
	const centres = $derived(cols.map((w, i) => lefts[i] + w / 2));
	const total = $derived(cols.reduce((a, b) => a + b, 0));
	/** A thick edge after column `i` where no transition joins it to the next. */
	const bold = (i: number): boolean => i < order.length - 1 && !joined(m, order[i], order[i + 1]);
	const beat = (s: Machine['states'][string]): string => (s.interval_expression ? 'ƒ' : s.interval < 0 ? 'never' : s.interval === 0 ? 'once' : `${s.interval.toFixed(2)}s`);
	const MODE = { sequence: 'seq', random: 'rand', expression: 'expr' } as const;

	// ---- the cells: a state's value of an attribute, or the default it shows unset.
	const valueOf = (state: string, attr: string): Literal => m.states[state]?.values?.[attr] ?? m.attributes[attr].default;
	const isSet = (state: string, attr: string): boolean => m.states[state]?.values?.[attr] !== undefined;
	const dims = (attr: string): number => {
		const d = m.attributes[attr].default;
		return Array.isArray(d) ? d.length : 1;
	};
	/** Whether the face paints under a drag: a number, a vector, a toggle or a pick among options. */
	const paintable = (a: Attribute): boolean => fractionOf(a, a.default) !== null || faceOf(a) === 'vector';

	// ---- a paint: a press on a cell sets it where the pointer is, a drag on across the columns
	// sets each cell it passes, and the release lands them all as ONE step. A selected column
	// paints every selected column with it.
	const cells: Record<string, HTMLElement | null> = {};
	const key = (attr: string, state: string): string => `${attr}/${state}`;
	let paint: { attr: string; touched: Map<string, Literal>; pointer: number } | null = null;
	function paintDown(e: PointerEvent, attr: string): void {
		if (e.button !== 0 || !paintable(m.attributes[attr])) return;
		paint = { attr, touched: new Map(), pointer: e.pointerId };
		(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
		e.preventDefault();
		paintAt(e);
	}
	function paintAt(e: PointerEvent): void {
		if (!paint) return;
		const a = m.attributes[paint.attr];
		for (const state of order) {
			const r = cells[key(paint.attr, state)]?.getBoundingClientRect();
			if (!r || e.clientX < r.left || e.clientX >= r.right) continue;
			const fraction = 1 - (e.clientY - r.top) / Math.max(1, r.height);
			const dim = Math.min(dims(paint.attr) - 1, Math.floor(((e.clientX - r.left) / Math.max(1, r.width)) * dims(paint.attr)));
			const targets = selected.includes(state) ? selected : [state];
			for (const t of targets) {
				const v = valueAt(a, fraction, valueOf(t, paint.attr), dim);
				if (v === null) continue;
				paint.touched.set(t, v);
				g.previewState(name, t, { values: { [paint.attr]: v } });
			}
			return;
		}
	}
	async function paintUp(): Promise<void> {
		const done = paint;
		paint = null;
		if (!done || done.touched.size === 0) return;
		try {
			await history().transaction(`Paint ${done.attr}`, async (step) => {
				for (const [state, v] of done.touched) await g.machine('machine state edit', { machine: name, name: state, values: { [done.attr]: v } }, step);
			});
		} catch (e) {
			notify().failure('Paint', e);
		}
	}

	// ---- a column's edge: dragged, it is the state's interval; a state with no beat takes one.
	function resizeDown(e: PointerEvent, i: number): void {
		const state = order[i];
		const w0 = cols[i];
		const d0 = duration(m, state);
		const sum = order.reduce((a, s) => a + duration(m, s), 0);
		const perSecond = sum > 0 ? total / sum : 100;
		let interval = d0;
		e.preventDefault();
		e.stopPropagation();
		beginDrag(e.currentTarget as HTMLElement, e.pointerId, {
			move: (p) => {
				const w = Math.max(SCORE.minColumn, w0 + (p.clientX - e.clientX));
				interval = Number((d0 > 0 ? (d0 * w) / w0 : w / perSecond).toFixed(2));
				g.previewState(name, state, { interval });
			},
			commit: () => void g.machine('machine state edit', { machine: name, name: state, interval }).catch((err) => notify().failure('Edit state', err)),
			cancel: () => g.previewState(name, state, { interval: m.states[state].interval })
		});
	}

	// ---- the selection: a column's head picks its state as a box does; the canvas's choice lights here.
	function pickState(e: MouseEvent, state: string): void {
		if (modifier(e)) sel.clickNode(panelId, state, true);
		else sel.setSelection(panelId, [state], []);
	}
	const lit = (state: string): boolean => hover.litState(name, state);

	// ---- the playheads: a dot at the head of the column each rests in, and a line sweeping the
	// column from its arrival to its next beat, timed from the frame that told of the arrival.
	const playheads = $derived(Object.keys(m.playheads));
	watchVariables(() => playheads.flatMap((p) => [`${p}.state`, `${p}.progress`]));
	const heads = $derived(
		playheads.map((p, i) => {
			const state = String(variableValue(`${p}.state`) ?? m.playheads[p].start);
			const progress = Number(variableValue(`${p}.progress`) ?? 1);
			const meta = variableFrame(`${p}.progress`)?.meta ?? {};
			const prev = String(meta.prev ?? '');
			const arrived = Math.max(Number(meta.arrived ?? 0), Number(variableFrame(`${p}.state`)?.meta.arrived ?? 0));
			return { p, color: dotColor(m.playheads[p].color, i), state, flying: prev !== '' && progress < 1, arrived };
		})
	);
	const landed = new Map<string, { arrived: number; at: number }>();
	let clock = $state(0);
	$effect(() => {
		for (const h of heads) {
			const was = landed.get(h.p);
			if (!was || was.arrived !== h.arrived) landed.set(h.p, { arrived: h.arrived, at: performance.now() });
		}
	});
	$effect(() => {
		if (playheads.length === 0) return;
		let frame = 0;
		const tick = (): void => {
			clock = performance.now();
			frame = requestAnimationFrame(tick);
		};
		frame = requestAnimationFrame(tick);
		return () => cancelAnimationFrame(frame);
	});
	/** Where a resting head's sweep stands across its column, 0..1; null with no beat to sweep to. */
	function sweep(h: (typeof heads)[number]): number | null {
		const s = m.states[h.state];
		const at = landed.get(h.p)?.at;
		if (h.flying || !s || !(s.interval > 0) || at === undefined) return null;
		return Math.min(1, (clock - at) / 1000 / s.interval);
	}
	const sweepX = (h: (typeof heads)[number]): number | null => {
		const i = order.indexOf(h.state);
		const f = sweep(h);
		return i < 0 || f === null ? null : lefts[i] + f * cols[i];
	};

	// ---- the popovers: a cell's value, a row's attribute, a column's state, an arc's transition.
	type Pick = { kind: 'cell'; attr: string; state: string } | { kind: 'attribute'; attr: string } | { kind: 'state'; ids: string[] } | { kind: 'transition'; id: string };
	let pick = $state<{ what: Pick; anchor: HTMLElement } | null>(null);
	const show = (what: Pick, e: Event): void => {
		pick = { what, anchor: (e.currentTarget as HTMLElement).closest('[data-anchor]') ?? (e.currentTarget as HTMLElement) };
	};
	/** The states a cell's popover writes to: every selected one when the cell's is among them. */
	const cellTargets = (state: string): string[] => (selected.includes(state) ? selected : [state]);
	const editCell = (state: string, attr: string, patch: Record<string, unknown>): void => void editEach(name, 'machine state edit', 'name', cellTargets(state), patch);
	const envelopeOf = (state: string, attr: string): number[] | null => m.states[state]?.envelopes?.[attr] ?? null;
	const LONGEST = 10;
	// The pick follows the document: a state or an attribute gone takes its popover with it, in
	// the same render — a cell's popover reads its attribute's default.
	const live = $derived.by(() => {
		const w = pick?.what;
		if (!w) return null;
		const gone =
			(w.kind === 'cell' && (!(w.state in m.states) || !(w.attr in m.attributes))) ||
			(w.kind === 'attribute' && !(w.attr in m.attributes)) ||
			(w.kind === 'state' && !w.ids.every((id) => id in m.states)) ||
			(w.kind === 'transition' && !(w.id in m.transitions));
		return gone ? null : pick;
	});
	// And it stays gone: a thing of the same name born later does not bring the popover back.
	$effect(() => {
		if (pick && !live) pick = null;
	});

	// ---- a new attribute, named at the foot: set in the selected states, as ONE step.
	let fresh = $state('');
	const creatable = $derived(isValidIdentifier(fresh.trim()) && !(fresh.trim() in m.attributes));
	async function createAttribute(): Promise<void> {
		const attr = fresh.trim();
		if (!creatable) return;
		try {
			await history().transaction(`Add attribute ${attr}`, async (step) => {
				await g.machine('machine attribute add', { machine: name, name: attr, value: FACES[0].born, kind: FACES[0].kind }, step);
				for (const id of selected) await g.machine('machine state edit', { machine: name, name: id, values: { [attr]: FACES[0].born } }, step);
			});
			fresh = '';
		} catch (e) {
			notify().failure('Add attribute', e);
		}
	}
	// ---- a new state, at the score's end: after the last column, a transition from it.
	async function appendState(): Promise<void> {
		const last = order[order.length - 1];
		try {
			await history().transaction('Add state', async (step) => {
				const r = await g.machine<{ name: string }>('machine state add', { machine: name }, step);
				if (last) await g.machine('machine transition add', { machine: name, from: last, to: r.name }, step);
				sel.setSelection(panelId, [r.name], []);
			});
		} catch (e) {
			notify().failure('Add state', e);
		}
	}
	function pickTransition(id: string, e: MouseEvent): void {
		sel.clickEdge(panelId, id, modifier(e));
		show({ kind: 'transition', id }, e);
	}
</script>

{#snippet cell(attr: string, state: string)}
	{@const a = m.attributes[attr]}
	{@const face = faceOf(a)}
	{@const v = valueOf(state, attr)}
	{@const set = isSet(state, attr)}
	{@const env = envelopeOf(state, attr)}
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class={`cell face-${face}`} class:unset={!set} class:paintable={paintable(a)} bind:this={cells[key(attr, state)]} data-anchor
		data-testid={`cell-${attr}-${state}`} data-set={set} data-envelope={env !== null} title={`${state}.${attr} = ${brief(v)}`}
		onpointerdown={(e) => paintDown(e, attr)} onpointermove={paintAt} onpointerup={paintUp} onpointercancel={paintUp}>
		{#if face === 'number' || face === 'toggle' || face === 'select'}
			<div class="fill" style={`height: ${(fractionOf(a, v) ?? 0) * 100}%`}></div>
		{:else if face === 'vector'}
			<div class="bars">
				{#each (v as number[]) as _, i (i)}
					<div class="bar"><div class="fill" style={`height: ${(fractionOf(a, v, i) ?? 0) * 100}%`}></div></div>
				{/each}
			</div>
		{:else if face === 'color'}
			<div class="ink" style={`background: ${colorCss(v as number[])}`}></div>
		{:else if face === 'envelope'}
			<svg class="shape" viewBox="0 0 100 40" preserveAspectRatio="none" aria-hidden="true">
				<path class="shape-fill" d={`${pathOf(envelopePoints(v as number[], 100, 40))} Z`} />
				<path class="shape-line" d={pathOf(envelopePoints(v as number[], 100, 40))} vector-effect="non-scaling-stroke" />
			</svg>
		{/if}
		{#if face === 'text'}
			<div class="text" data-testid="cell-text">{String(v)}</div>
		{:else}
			<span class="readout" data-testid="cell-readout">{brief(v)}</span>
		{/if}
		{#if env}
			<span class="env" title="Plays an envelope on arrival">adsr</span>
		{/if}
		<span class="cog" onpointerdown={(e) => e.stopPropagation()}>
			<button type="button" class="cog-btn" aria-label={`${state} ${attr} settings`} title="The value with its own widget, its envelope, and unset" data-testid="cell-settings"
				onclick={(e) => show({ kind: 'cell', attr, state }, e)}><Icon name="settings" /></button>
		</span>
	</div>
{/snippet}

<div class="score" data-testid="machine-inspector" data-count={selected.length} data-open={subject.open.join(' ')} data-order={order.join(' ')} bind:clientWidth={scoreW}>
	{#if order.length === 0}
		<div class="empty">
			<EmptyState>
				{#snippet title()}No states yet{/snippet}
				{#snippet hint()}Double-click the canvas, or add one here.{/snippet}
			</EmptyState>
			<Button size="sm" data-testid="score-add-state" onclick={() => void appendState()}><Icon name="plus" />state</Button>
		</div>
	{:else}
		<div class="sheet" style={`width: ${SCORE.gutter + total + SCORE.add}px`}>
			<!-- The lane: the transitions as arcs over the columns, its gutter holding the ✕. -->
			<div class="line lane-line" style={`height: ${SCORE.lane}px`}>
				<div class="gutter lane-gutter">
					<IconButton variant="ghost" size="sm" label="Close inspector" title="Close the inspector" data-testid="inspector-close" onclick={onClose}><Icon name="x" /></IconButton>
				</div>
				<TransitionLane {name} {m} {order} {centres} width={total} selected={picked} onPick={pickTransition} />
			</div>
			<!-- The head: each state's name, beat and decision, its dots, its cog, and its edge. -->
			<div class="line head-line" style={`height: ${SCORE.head}px`}>
				<div class="gutter head-gutter"><span class="muted">states</span></div>
				{#each order as state, i (state)}
					{@const s = m.states[state]}
					<!-- svelte-ignore a11y_no_static_element_interactions -->
					<div class="head" class:selected={selected.includes(state)} class:lit={lit(state)} class:bold={bold(i)} style={`width: ${cols[i]}px`} data-anchor
						data-testid={`column-${state}`} data-selected={selected.includes(state)}
						onpointerenter={() => hover.overState(name, state)} onpointerleave={() => hover.clear()}>
						<button type="button" class="pick" title="Select the state; shift adds it to the selection" data-testid="column-pick" onclick={(e) => pickState(e, state)}>
							<span class="name">{state}</span>
							<span class="facts">{beat(s)} · {MODE[s.mode]}</span>
						</button>
						<span class="dots">
							{#each heads.filter((h) => h.state === state) as h (h.p)}
								<span class="dot" class:flying={h.flying} style={`background: ${h.color}`} title={h.p} data-testid="score-dot" data-playhead={h.p} data-flying={h.flying}></span>
							{/each}
						</span>
						<span class="cog head-cog">
							<button type="button" class="cog-btn" aria-label={`${state} settings`} title="How it decides and how often, its playheads and transitions" data-testid="column-settings"
								onclick={(e) => show({ kind: 'state', ids: selected.includes(state) && selected.length > 1 ? selected : [state] }, e)}><Icon name="settings" /></button>
						</span>
						<!-- svelte-ignore a11y_no_static_element_interactions -->
						<span class="edge" title="Drag: the state's interval" data-testid="column-edge" onpointerdown={(e) => resizeDown(e, i)}></span>
					</div>
				{/each}
				<div class="add-col">
					<IconButton variant="ghost" size="sm" label="Add a state" title="Add a state after the last column, with a transition from it" data-testid="score-add-state" onclick={() => void appendState()}><Icon name="plus" /></IconButton>
				</div>
			</div>
			<!-- The rows: one per attribute, its head in the gutter and a cell per column. -->
			{#each attrs as attr (attr)}
				{@const a = m.attributes[attr]}
				<div class="line row-line" style={`height: ${SCORE.row}px`} data-testid={`row-${attr}`}>
					<div class="gutter row-gutter" data-anchor>
						<span class="row-name" title={attr}>{attr}</span>
						<svg class="curve-glyph" viewBox="-1 -1 26 18" aria-hidden="true"><path d={curvePath(a.curve, 24, 16, 24)} vector-effect="non-scaling-stroke" /></svg>
						<span class="cog">
							<button type="button" class="cog-btn" aria-label={`${attr} definition`} title="Its name, kind, range, default and curve, for every state" data-testid="row-settings"
								onclick={(e) => show({ kind: 'attribute', attr }, e)}><Icon name="settings" /></button>
						</span>
					</div>
					{#each order as state, i (state)}
						<!-- svelte-ignore a11y_no_static_element_interactions -->
						<div class="col" class:selected={selected.includes(state)} class:lit={lit(state)} class:bold={bold(i)} style={`width: ${cols[i]}px`}
							onpointerenter={() => hover.overState(name, state)} onpointerleave={() => hover.clear()}>
							{@render cell(attr, state)}
						</div>
					{/each}
				</div>
			{/each}
			<!-- The foot: a new attribute, named. -->
			<div class="line foot-line">
				<div class="gutter foot-gutter">
					<TextInput class="fresh" inputmode="search" placeholder="attribute…" aria-label="New attribute" data-testid="attribute-name-new"
						value={fresh} onInput={(v) => (fresh = v)} onChange={(v) => { fresh = v; void createAttribute(); }} />
					<IconButton variant="ghost" size="sm" label="Add the attribute" title="Add an attribute of that name, set in the selected states" disabled={!creatable}
						data-testid="attribute-create" onclick={() => void createAttribute()}><Icon name="plus" /></IconButton>
				</div>
			</div>
			<!-- The sweeps: a line per resting playhead, over the rows of its column. -->
			<div class="sweeps" style={`left: ${SCORE.gutter}px; top: ${SCORE.lane + SCORE.head}px; height: ${attrs.length * SCORE.row}px`}>
				{#each heads as h (h.p)}
					{@const x = sweepX(h)}
					{#if x !== null}
						<div class="sweep" style={`left: ${x}px; background: ${h.color}`} data-testid="score-sweep" data-playhead={h.p}></div>
					{/if}
				{/each}
			</div>
		</div>
	{/if}
</div>

{#if live}
	{@const w = live.what}
	<Popover anchor={live.anchor} open onDismiss={() => (pick = null)} catcher flip role="dialog" aria-label="settings" style="--popover-min-width: 18rem; --popover-max-width: 24rem" data-testid="score-popover" data-kind={w.kind}>
		<div class="pop">
			{#if w.kind === 'cell'}
				{@const a = m.attributes[w.attr]}
				{@const targets = cellTargets(w.state)}
				{@const v = valueOf(w.state, w.attr)}
				{@const env = envelopeOf(w.state, w.attr)}
				<div class="pop-head">
					<span class="mono">{targets.length > 1 ? `${targets.length} states` : w.state}</span>
					<span class="muted">·</span>
					<span class="mono">{w.attr}</span>
					<Button variant="ghost" size="sm" title="Unset: a playhead entering keeps what it holds" disabled={!isSet(w.state, w.attr)} data-testid="cell-unset"
						onclick={() => editCell(w.state, w.attr, { values: { [w.attr]: null } })}>unset</Button>
				</div>
				<ParamField paramName={w.attr} descriptor={descriptorFor(a, v)} literal viewKey={`${name}/cell/${w.attr}`}
					onCommit={(x) => editCell(w.state, w.attr, { values: { [w.attr]: literalOf(x) } })}
					onPreview={(x) => targets.forEach((t) => g.previewState(name, t, { values: { [w.attr]: x } }))} />
				{#if a.kind.type === 'num'}
					<div class="fields">
						<Field label="envelope" doc="On arrival the value plays an attack, decay, sustain and release" row>
							<Toggle value={env !== null} disabled={!isSet(w.state, w.attr)} aria-label="envelope" data-testid="state-envelope"
								onChange={(on) => editCell(w.state, w.attr, { envelopes: { [w.attr]: on ? ENVELOPE_BORN : null } })} />
						</Field>
					</div>
					{#if env}
						<ParamField paramName={`${w.attr} envelope`} label="shape" descriptor={envelopeDescriptor(env, LONGEST)} literal viewKey={`${name}/cell/${w.attr}/envelope`}
							data-testid="state-attribute-envelope"
							onCommit={(x) => editCell(w.state, w.attr, { envelopes: { [w.attr]: x } })}
							onPreview={(x) => targets.forEach((t) => g.previewState(name, t, { envelopes: { [w.attr]: x as number[] } }))} />
					{/if}
				{/if}
			{:else if w.kind === 'attribute'}
				<AttributeForm {name} attr={w.attr} a={m.attributes[w.attr]} />
			{:else if w.kind === 'state'}
				<StateForm {name} {m} ids={w.ids} open={subject.open} />
			{:else}
				{@const t = m.transitions[w.id]}
				<div class="pop-head">
					<span class="mono">{t.from === '*' ? 'any' : t.from} → {t.to}</span>
				</div>
				<TransitionForm {name} {m} id={w.id} />
			{/if}
		</div>
	</Popover>
{/if}

<style>
	.score {
		position: relative;
		flex: 1;
		min-width: 0;
		min-height: 0;
		overflow: auto;
		background: var(--surface-1);
		font-size: var(--fs-small);
		user-select: none;
	}
	.empty {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: var(--space-4);
		padding: var(--space-8);
	}
	.sheet {
		position: relative;
		display: flex;
		flex-direction: column;
	}
	.line {
		display: flex;
		align-items: stretch;
		flex: 0 0 auto;
	}
	/* The gutter holds a line's head and stays put while the columns scroll under it. */
	.gutter {
		position: sticky;
		left: 0;
		z-index: 2;
		flex: 0 0 auto;
		display: flex;
		align-items: center;
		gap: var(--space-2);
		width: var(--gutter);
		box-sizing: border-box;
		padding: 0 var(--space-3);
		background: var(--surface-1);
		border-right: 1px solid var(--border);
	}
	.sheet {
		--gutter: 112px;
	}
	.lane-gutter {
		justify-content: flex-end;
	}
	.head-line,
	.row-line {
		border-bottom: 1px solid var(--border);
	}
	.foot-line {
		height: var(--hit);
	}
	.foot-gutter {
		width: calc(var(--gutter) + 6rem);
		border-right: none;
	}
	.foot-gutter > :global(.fresh) {
		flex: 1 1 auto;
		min-width: 0;
	}
	.muted {
		color: var(--text-muted);
	}
	.mono {
		font-family: var(--font-mono);
	}
	/* A column's head: the name over its facts, the dots at its top edge, the cog under the pointer,
	   and the edge that drags its interval. */
	.head,
	.col {
		position: relative;
		flex: 0 0 auto;
		box-sizing: border-box;
		border-right: 1px solid var(--border);
		transition: background var(--dur-fast) var(--ease);
	}
	.head.bold,
	.col.bold {
		border-right: 3px solid var(--border-strong);
	}
	.head.lit,
	.col.lit {
		background: var(--hover-fill);
	}
	.head.selected,
	.col.selected {
		background: var(--accent-fill);
	}
	.head.selected {
		box-shadow: inset 0 2px 0 var(--accent);
	}
	.pick {
		display: flex;
		flex-direction: column;
		justify-content: center;
		gap: var(--space-1);
		width: 100%;
		height: 100%;
		padding: 0 var(--space-3);
		border: 0;
		background: none;
		color: var(--text);
		font: inherit;
		text-align: left;
		cursor: pointer;
	}
	.pick:focus-visible {
		outline: var(--focus-width) solid var(--focus-ink);
		outline-offset: -2px;
	}
	.name {
		font-family: var(--font-mono);
		font-weight: 600;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.facts {
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		font-variant-numeric: tabular-nums;
		color: var(--text-muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.dots {
		position: absolute;
		top: 3px;
		left: var(--space-3);
		display: flex;
		gap: var(--space-1);
		pointer-events: none;
	}
	.dot {
		width: 9px;
		height: 9px;
		border-radius: 50%;
	}
	.dot.flying {
		opacity: 0.5;
	}
	/* A cog in the corner, small enough that a column's or a cell's middle is never under it. */
	.cog {
		position: absolute;
		right: 2px;
		bottom: 2px;
		opacity: 0;
		transition: opacity var(--dur-fast) var(--ease);
	}
	.cog-btn {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: var(--chrome-control-h);
		height: var(--chrome-control-h);
		padding: 0;
		border: 0;
		border-radius: var(--radius-sm);
		background: var(--surface-2);
		color: var(--text-dim);
		cursor: pointer;
	}
	.cog-btn:hover {
		color: var(--text);
		background: var(--surface-3);
	}
	.cog-btn:focus-visible {
		outline: var(--focus-width) solid var(--focus-ink);
	}
	.head:hover .cog,
	.cell:hover .cog,
	.row-gutter:hover .cog,
	.cog:focus-within {
		opacity: 1;
	}
	.edge {
		position: absolute;
		top: 0;
		bottom: 0;
		right: -4px;
		width: 8px;
		cursor: col-resize;
		touch-action: none;
		z-index: 1;
	}
	.add-col {
		flex: 0 0 36px;
		display: flex;
		align-items: center;
		justify-content: center;
	}
	.row-gutter {
		position: relative;
	}
	.row-name {
		flex: 1 1 auto;
		min-width: 0;
		font-family: var(--font-mono);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.curve-glyph {
		flex: 0 0 auto;
		width: 1.4em;
		height: 1em;
		fill: none;
		stroke: var(--text-muted);
		stroke-width: 1.5;
	}
	/* A cell: the fill from the bottom is the value's height, the readout rides its top, a text
	   wraps and scrolls in it, and a cog waits in its corner. */
	.cell {
		position: relative;
		width: 100%;
		height: 100%;
		overflow: hidden;
		touch-action: none;
	}
	.cell.paintable {
		cursor: ns-resize;
	}
	.cell.unset {
		opacity: 0.45;
	}
	.fill {
		position: absolute;
		left: 0;
		right: 0;
		bottom: 0;
		background: var(--accent-fill);
		border-top: 2px solid var(--accent);
		pointer-events: none;
	}
	.bars {
		position: absolute;
		inset: 0;
		display: flex;
		gap: 1px;
	}
	.bar {
		position: relative;
		flex: 1 1 0;
	}
	.ink {
		position: absolute;
		inset: var(--space-2);
		border-radius: var(--radius-sm);
		border: 1px solid var(--border);
	}
	.shape {
		position: absolute;
		inset: var(--space-2);
		width: calc(100% - 2 * var(--space-2));
		height: calc(100% - 2 * var(--space-2));
	}
	.shape-fill {
		fill: var(--accent-fill);
	}
	.shape-line {
		fill: none;
		stroke: var(--accent);
		stroke-width: 1.5;
	}
	.readout {
		position: absolute;
		top: 2px;
		left: var(--space-2);
		right: 2.4em;
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		pointer-events: none;
	}
	.text {
		position: absolute;
		inset: var(--space-2);
		overflow: auto;
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		user-select: text;
	}
	.env {
		position: absolute;
		top: 2px;
		right: var(--space-2);
		padding: 0 var(--space-1);
		border-radius: var(--radius-sm);
		background: var(--accent);
		color: var(--on-accent);
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		line-height: 1.4;
		pointer-events: none;
	}
	.sweeps {
		position: absolute;
		pointer-events: none;
	}
	.sweep {
		position: absolute;
		top: 0;
		bottom: 0;
		width: 2px;
		opacity: 0.8;
	}
	/* The popovers: a form on the pane's step, the inspector's field rows. */
	.pop {
		display: flex;
		flex-direction: column;
		gap: var(--space-3);
		width: 22rem;
		max-width: 100%;
	}
	.pop :global(.fields) {
		display: grid;
		grid-template-columns: max-content minmax(0, 1fr);
		column-gap: var(--space-5);
		row-gap: var(--space-3);
		--number-width: 100%;
	}
	.pop-head {
		display: flex;
		align-items: center;
		gap: var(--space-2);
		min-height: var(--chrome-control-h);
		font-weight: 600;
	}
	.pop-head > :global(button:last-child) {
		margin-left: auto;
	}
	@media (hover: none) and (pointer: coarse) {
		.cog {
			opacity: 1;
		}
	}
</style>
