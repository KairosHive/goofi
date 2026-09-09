<!-- Application logs and the shared op command interface. -->
<script lang="ts">
	import { getControl } from '$lib/api/control';
	import type { PanelProps } from 'panelty';
	import { consoleStore, type ConsoleEntry, type LogLevel } from '$lib/stores/console.svelte';
	import { selection } from '$lib/stores/selection.svelte';
	import { ui } from '$lib/stores/ui.svelte';
	import { graph } from '$lib/stores/graph.svelte';
	import { linkedNodeName } from 'panelty';
	import { copyText } from '$lib/clipboard';
	import { COLLAPSE_LINES, estimateRowHeight } from './consoleRowHeight';
	import NodeSelect from './NodeSelect.svelte';
	import { Bar, Chip, Badge, Icon, IconButton, EmptyState } from '$lib/ui';
	import { onDestroy, tick } from 'svelte';

	let { panelId, state: linkState }: PanelProps = $props();
	const sel = selection();
	const uiStore = ui();
	const cs = consoleStore();

	const filterName = $derived(linkedNodeName(linkState)); // the bound node's uid (identity)
	const nodeLabel = (uid: string): string => graph().nodeById(uid)?.name ?? uid;
	const dragActive = $derived(uiStore.nodeDrag !== null);
	const over = $derived(uiStore.nodeDragTarget === panelId);

	let levels = $state(new Set<LogLevel>(['info', 'warning', 'error']));
	let query = $state('');
	let command = $state('');
	let busy = $state(false);
	let commandError = $state('');
	let history: string[] = [];
	let historyIndex = 0;
	let draft = '';
	let completions = $state<string[]>([]);

	function toggleLevel(level: LogLevel): void {
		const next = new Set(levels);
		if (next.has(level)) next.delete(level); else next.add(level);
		levels = next;
	}

	async function submit(): Promise<void> {
		const line = command.trim();
		if (!line || busy) return;
		busy = true;
		commandError = '';
		history = [...history.filter((entry) => entry !== line), line].slice(-200);
		historyIndex = history.length;
		command = '';
		completions = [];
		try {
			await getControl().call('log write', { text: `› ${line}`, component: 'command' });
			const response = await fetch('/exec', {
				method: 'POST', headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify({ commands: [line], actor: getControl().actor })
			});
			const result = await response.json();
			if (!response.ok || result.error) throw new Error(result.error ?? `Request failed (${response.status})`);
			for (const entry of result.results) {
				await getControl().call('log write', { text: entry.text, component: 'command' });
			}
		} catch (error) {
			const text = error instanceof Error ? error.message : String(error);
			try { await getControl().call('log write', { text, level: 'error', component: 'command' }); }
			catch { commandError = text; }
		} finally { busy = false; }
	}

	async function commandKey(event: KeyboardEvent): Promise<void> {
		if (event.key === 'ArrowUp' || event.key === 'ArrowDown') {
			event.preventDefault();
			if (historyIndex === history.length) draft = command;
			historyIndex = Math.max(0, Math.min(history.length, historyIndex + (event.key === 'ArrowUp' ? -1 : 1)));
			command = history[historyIndex] ?? draft;
		} else if (event.key === 'Tab') {
			event.preventDefault();
			const line = command;
			try {
				const result = await getControl().call<{ text: string }>('op complete', { line });
				if (command !== line) return;
				completions = result.text.split('\n').filter(Boolean).map((row) => row.split('\t')[0]);
				if (completions.length === 1) {
					command = line.replace(/[^\s]*$/, completions[0]) + ' ';
					completions = [];
				}
			} catch (error) { commandError = String(error); }
		} else if (event.key === 'Escape') { completions = []; }
	}

	async function clearLogs(): Promise<void> {
		try { await getControl().call('log clear'); commandError = ''; }
		catch (error) { commandError = String(error); }
	}

	const OVERSCAN = 8;

	// Panel-local: wrapped heights depend on *this* panel's width, so they can't live in the store.
	let expanded = $state(new Set<number>());
	let measured = $state(new Map<number, { h: number; trunc: boolean }>());

	/** The row's content floor in px, read from the same token and query the CSS floors with. */
	function contentFloor(): number {
		if (typeof window === 'undefined') return 0;
		if (!window.matchMedia('(hover: none) and (pointer: coarse)').matches) return 0;
		return parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--hit')) || 0;
	}
	function heightOf(e: ConsoleEntry, floor: number): number {
		return measured.get(e.uid)?.h ?? estimateRowHeight(e.lines, expanded.has(e.uid), floor);
	}
	function expandable(e: ConsoleEntry): boolean {
		return measured.get(e.uid)?.trunc ?? e.lines > COLLAPSE_LINES;
	}

	// ResizeObserver fires after layout, so writing `measured` here can't recurse into the size.
	function measure(node: HTMLElement, params: { uid: number; exp: boolean }) {
		let cur = params;
		const report = (): void => {
			const h = node.offsetHeight;
			const txt = node.querySelector('.txt');
			const trunc = !cur.exp && txt ? txt.scrollHeight - txt.clientHeight > 1 : undefined;
			const prev = measured.get(cur.uid);
			const next = { h, trunc: trunc ?? prev?.trunc ?? false };
			if (!prev || prev.h !== next.h || prev.trunc !== next.trunc) {
				const m = new Map(measured);
				m.set(cur.uid, next);
				measured = m;
			}
		};
		const ro = new ResizeObserver(report);
		ro.observe(node);
		report();
		return {
			update(next: { uid: number; exp: boolean }) {
				cur = next;
				report();
			},
			destroy: () => ro.disconnect()
		};
	}

	function toggle(uid: number): void {
		const next = new Set(expanded);
		if (next.has(uid)) next.delete(uid);
		else next.add(uid);
		expanded = next;
	}

	// A text-selection drag ends with a click on the row; only a stationary click toggles it.
	let downX = 0;
	let downY = 0;
	function onRowDown(ev: MouseEvent): void {
		downX = ev.clientX;
		downY = ev.clientY;
	}
	function onRowClick(ev: MouseEvent, uid: number, canToggle: boolean): void {
		if (!canToggle) return;
		if (Math.hypot(ev.clientX - downX, ev.clientY - downY) > 4) return;
		toggle(uid);
	}

	let copiedUid = $state(-1);
	let copiedTimer: ReturnType<typeof setTimeout> | undefined;
	async function copy(text: string, uid: number): Promise<void> {
		if (!(await copyText(text))) {
			console.warn('clipboard copy failed');
			return;
		}
		copiedUid = uid;
		clearTimeout(copiedTimer);
		copiedTimer = setTimeout(() => (copiedUid = -1), 1000);
	}

	onDestroy(() => {
		clearTimeout(copiedTimer);
	});

	const view = $derived.by(() => {
		cs.version;
		return cs.view(filterName, levels, query);
	});

	$effect(() => {
		const ids = new Set(Array.from({ length: view.total() }, (_, i) => view.get(i).uid));
		const kept = new Map([...measured].filter(([id]) => ids.has(id)));
		if (kept.size !== measured.size) measured = kept;
		const open = new Set([...expanded].filter((id) => ids.has(id)));
		if (open.size !== expanded.size) expanded = open;
	});

	let scrollEl = $state<HTMLDivElement | null>(null);
	let scrollTop = $state(0);
	let viewportH = $state(0);
	let stuck = $state(true); // pinned to the bottom until the user scrolls up

	// Cumulative row offsets: cum[i] = total height of rows [0, i).
	const layout = $derived.by<{ n: number; cum: Float64Array; height: number }>(() => {
		cs.version;
		measured;
		expanded;
		const v = view;
		const n = v ? v.total() : 0;
		const cum = new Float64Array(n + 1);
		const floor = contentFloor();
		for (let i = 0; i < n; i++) cum[i + 1] = cum[i] + heightOf(v!.get(i), floor);
		return { n, cum, height: cum[n] };
	});

	// Largest index i with cum[i] <= y.
	function indexAt(cum: Float64Array, y: number): number {
		let lo = 0;
		let hi = cum.length - 1;
		while (lo < hi) {
			const mid = (lo + hi + 1) >> 1;
			if (cum[mid] <= y) lo = mid;
			else hi = mid - 1;
		}
		return lo;
	}

	const start = $derived(Math.max(0, indexAt(layout.cum, scrollTop) - OVERSCAN));
	const end = $derived(Math.min(layout.n, indexAt(layout.cum, scrollTop + viewportH) + OVERSCAN + 1));
	// Shallow-copy each visible entry: `count` is bumped in place on coalesce, and the keyed
	// {#each} would not re-render a same-reference item.
	const windowRows = $derived.by<{ e: ConsoleEntry; exp: boolean; canToggle: boolean }[]>(() => {
		cs.version;
		const v = view;
		if (!v) return [];
		const out: { e: ConsoleEntry; exp: boolean; canToggle: boolean }[] = [];
		const e = Math.min(end, v.total());
		for (let i = start; i < e; i++) {
			const copy = { ...v.get(i) };
			const exp = expanded.has(copy.uid);
			out.push({ e: copy, exp, canToggle: exp || expandable(copy) });
		}
		return out;
	});
	const topPad = $derived(layout.cum[Math.min(start, layout.n)]);
	const bottomPad = $derived(Math.max(0, layout.height - layout.cum[Math.min(end, layout.n)]));

	function onScroll(): void {
		if (!scrollEl) return;
		scrollTop = scrollEl.scrollTop;
		const dist = scrollEl.scrollHeight - scrollEl.scrollTop - scrollEl.clientHeight;
		stuck = dist < 24;
	}

	function scrollToBottom(): void {
		if (!scrollEl) return;
		scrollEl.scrollTop = scrollEl.scrollHeight;
		stuck = true;
	}

	$effect(() => {
		layout;
		if (stuck && scrollEl) {
			void tick().then(() => {
				if (scrollEl && stuck) scrollEl.scrollTop = scrollEl.scrollHeight;
			});
		}
	});

	function focus(name: string): void {
		if (sel.activeEditorId) sel.selectNodes(sel.activeEditorId, [name]);
	}
</script>

<div class="wrap" data-testid="console-panel">
	<Bar>
		{#snippet start()}
			{#each ['info', 'warning', 'error'] as level}
				<Chip density="chrome" tone={levels.has(level as LogLevel) ? 'accent' : 'neutral'}
					aria-pressed={levels.has(level as LogLevel)} onclick={() => toggleLevel(level as LogLevel)}
					title="Show {level} messages">{#if level === 'info'}<Icon name="info" />{/if}{level}</Chip>
			{/each}
			<IconButton variant="ghost" density="chrome" label="Clear console" title="Clear console" onclick={clearLogs}><Icon name="x" /></IconButton>
		{/snippet}
		{#snippet end()}
			<NodeSelect {panelId} state={linkState} emptyLabel="All sources" />
		{/snippet}
	</Bar>

	<input class="filter" aria-label="Filter messages" placeholder="Filter messages" bind:value={query} />

	<div
		class="scroll thin-scrollbar"
		bind:this={scrollEl}
		bind:clientHeight={viewportH}
		onscroll={onScroll}
	>
		{#if layout.n === 0}
			<EmptyState>
				{#snippet hint()}No output{filterName ? ' for this node' : ''} yet.{/snippet}
			</EmptyState>
		{:else}
			<div style="height:{topPad}px"></div>
			{#each windowRows as row (row.e.uid)}
				<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
				<div
					class="row"
					class:err={row.e.level === 'error'}
					class:warn={row.e.level === 'warning'}
					class:toggleable={row.canToggle}
					data-testid="console-entry"
					data-node={row.e.node}
					data-stream={row.e.stream}
					data-level={row.e.level}
					role={row.canToggle ? 'button' : undefined}
					tabindex={row.canToggle ? 0 : undefined}
					onmousedown={onRowDown}
					onclick={(ev) => onRowClick(ev, row.e.uid, row.canToggle)}
					onkeydown={(ev) => {
						if (row.canToggle && (ev.key === 'Enter' || ev.key === ' ')) {
							ev.preventDefault();
							toggle(row.e.uid);
						}
					}}
					use:measure={{ uid: row.e.uid, exp: row.exp }}
				>
					<span class="caret"
						>{#if row.exp}<Icon name="chevron-down" />{:else if row.canToggle}<Icon
								name="chevron-right"
							/>{/if}</span
					>
					{#if !filterName}
						<button
							class="node"
							onclick={(ev) => {
								ev.stopPropagation();
								if (row.e.node) focus(row.e.node);
							}}>{row.e.node ? nodeLabel(row.e.node) : row.e.component}</button
						>
					{/if}
					<time title={new Date(row.e.ts).toISOString()}>{new Date(row.e.ts).toLocaleTimeString()}</time>
					<pre class="txt" class:clamp={!row.exp}>{row.e.text}</pre>
					<div class="actions">
						{#if row.e.count > 1}
							<Badge data-testid="console-count" title="{row.e.count} occurrences"
								>×{row.e.count}</Badge
							>
						{/if}
						<IconButton
							class="console-copy-btn"
							variant="ghost"
							size="sm"
							density="chrome"
							data-testid="console-copy"
							title="Copy message"
							label="Copy message"
							onmousedown={(ev) => ev.stopPropagation()}
							onclick={(ev) => {
								ev.stopPropagation();
								copy(row.e.text, row.e.uid);
							}}><Icon name={copiedUid === row.e.uid ? 'check' : 'copy'} /></IconButton
						>
					</div>
				</div>
			{/each}
			<div style="height:{bottomPad}px"></div>
		{/if}
	</div>

	{#if !stuck && layout.n > 0}
		<IconButton
			class="to-bottom-fab"
			data-testid="console-to-bottom"
			title="Scroll to bottom"
			label="Scroll to bottom"
			onclick={scrollToBottom}>↓</IconButton
		>
	{/if}

	{#if completions.length}<div class="completions">{completions.join(' · ')}</div>{/if}
	{#if commandError}<div class="command-error" role="alert">{commandError}</div>{/if}
	<form class="prompt" onsubmit={(event) => { event.preventDefault(); void submit(); }}>
		<span aria-hidden="true">›</span>
		<input aria-label="Console command" placeholder="Enter an op · Tab to complete" bind:value={command}
			onkeydown={commandKey} autocomplete="off" spellcheck="false" />
		<button type="submit" disabled={busy || !command.trim()}>{busy ? 'Running…' : 'Run'}</button>
	</form>

	{#if dragActive}
		<div class="node-drop-hint" class:active={over} data-testid="node-drop-hint"></div>
	{/if}
</div>

<style>
	.filter, .prompt {
		border: 0;
		border-bottom: 1px solid var(--border);
		padding: var(--space-5) var(--space-6);
		background: transparent;
		color: var(--text);
		min-width: 0;
	}
	.prompt { display: flex; gap: var(--space-5); border-top: 1px solid var(--border); }
	.prompt input { flex: 1; min-width: 0; border: 0; background: transparent; color: inherit; font-family: var(--font-mono); }
	.prompt button { color: var(--accent); background: transparent; border: 0; cursor: pointer; }
	.prompt button:disabled { opacity: 0.5; }
	.command-error { color: var(--danger); }
	.command-error, .completions { padding: var(--space-5); overflow-wrap: anywhere; max-height: 100px; overflow: auto; }
	time { color: var(--text-muted); white-space: nowrap; font-size: var(--fs-micro); line-height: 16px; }
	.row.warn { color: var(--warning); background: color-mix(in srgb, var(--warning) 9%, transparent); }

	.wrap {
		position: relative;
		container-type: inline-size;
		height: 100%;
		display: flex;
		flex-direction: column;
		min-height: 0;
	}
	/* A native div, not ScrollArea: the virtual scroller keeps its own DOM handle. */
	.scroll {
		flex: 1;
		overflow-y: auto;
		overflow-x: hidden;
		min-height: 0;
		font-family: var(--font-mono);
		font-size: var(--fs-small);
	}
	.row {
		display: flex;
		align-items: flex-start;
		gap: var(--space-5);
		/* Mirrored by `PAD = 4` in consoleRowHeight.ts; px, because that estimate precedes layout. */
		padding: 2px var(--space-6);
		border-bottom: 1px solid color-mix(in srgb, var(--border) 55%, transparent);
		box-sizing: border-box;
	}
	.row.toggleable {
		cursor: pointer;
	}
	.row.toggleable:hover {
		background: color-mix(in srgb, var(--accent) 7%, transparent);
	}
	.row.err {
		background: color-mix(in srgb, var(--danger) 9%, transparent);
		color: var(--danger);
	}
	.row.err.toggleable:hover {
		background: var(--danger-fill);
	}
	.caret {
		flex: 0 0 auto;
		width: 10px;
		line-height: 16px;
		color: var(--text-muted);
		font-size: var(--fs-micro);
	}
	.node {
		flex: 0 0 auto;
		background: transparent;
		border: none;
		padding: 0;
		line-height: 16px;
		color: var(--accent);
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		cursor: pointer;
		max-width: 160px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.txt {
		flex: 1 1 auto;
		min-width: 0;
		margin: 0;
		font-size: var(--fs-small);
		line-height: 16px;
		white-space: pre-wrap;
		word-break: break-word;
		overflow: hidden;
		color: inherit;
		user-select: text;
		cursor: text;
	}
	.txt.clamp {
		display: -webkit-box;
		-webkit-line-clamp: 3;
		line-clamp: 3;
		-webkit-box-orient: vertical;
	}
	.actions {
		flex: 0 0 auto;
		display: flex;
		align-items: center;
		gap: var(--space-2);
	}
	/* Always occupies its slot, so hover reflows nothing and `estimateRowHeight`'s model holds. */
	.row :global(.console-copy-btn) {
		--panelty-icon-btn-size: 16px;
		opacity: 0;
		pointer-events: none;
		transition: opacity var(--dur-fast) var(--ease);
	}
	.row:hover :global(.console-copy-btn),
	.row:focus-within :global(.console-copy-btn) {
		opacity: 1;
		pointer-events: auto;
	}
	@container (max-width: 420px) {
		time { display: none; }
		.node { max-width: 90px; }
	}
	/* Touch has no hover, so the copy button rests open. */
	@media (hover: none) and (pointer: coarse) {
		.row :global(.console-copy-btn) {
			opacity: 1;
			pointer-events: auto;
		}
	}
	.wrap :global(.to-bottom-fab) {
		position: absolute;
		right: 12px;
		bottom: 52px;
		border-radius: 999px;
		box-shadow: var(--shadow-1);
		z-index: 2;
	}
</style>
