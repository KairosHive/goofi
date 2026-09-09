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
	import { estimateRowHeight } from './consoleRowHeight';
	import NodeSelect from './NodeSelect.svelte';
	import { Bar, Chip, Badge, Icon, IconButton, EmptyState } from '$lib/ui';
	import { onDestroy, tick } from 'svelte';

	let { panelId, state: linkState }: PanelProps = $props();
	const sel = selection();
	const uiStore = ui();
	const cs = consoleStore();

	const filterName = $derived(linkedNodeName(linkState)); // the bound node's uid (identity)
	const nodeLabel = (uid: string): string => graph().nodeById(uid)?.name ?? uid;
	const timeFormat = new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' });
	const sourceLabel = (entry: ConsoleEntry): string => entry.node ? nodeLabel(entry.node) : entry.component;
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

	const OVERSCAN = 8;

	// Panel-local: wrapped heights depend on *this* panel's width, so they can't live in the store.
	let measured = $state(new Map<number, number>());

	/** The row's content floor in px, read from the same token and query the CSS floors with. */
	function contentFloor(): number {
		if (typeof window === 'undefined') return 0;
		if (!window.matchMedia('(hover: none) and (pointer: coarse)').matches) return 0;
		return parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--hit')) || 0;
	}
	function heightOf(e: ConsoleEntry, floor: number): number {
		return measured.get(e.uid) ?? estimateRowHeight(e.lines, floor);
	}

	// Wrapped text needs a measured height for this panel's width.
	function measure(node: HTMLElement, uid: number) {
		let id = uid;
		const report = (): void => {
			const h = node.offsetHeight;
			if (measured.get(id) !== h) {
				const next = new Map(measured);
				next.set(id, h);
				measured = next;
			}
		};
		const ro = new ResizeObserver(report);
		ro.observe(node);
		report();
		return {
			update(uid: number) { id = uid; report(); },
			destroy: () => ro.disconnect()
		};
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
	});

	let scrollEl = $state<HTMLDivElement | null>(null);
	let scrollTop = $state(0);
	let viewportH = $state(0);
	let stuck = $state(true); // pinned to the bottom until the user scrolls up

	// Cumulative row offsets: cum[i] = total height of rows [0, i).
	const layout = $derived.by<{ n: number; cum: Float64Array; height: number }>(() => {
		cs.version;
		measured;
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
	const windowRows = $derived.by<ConsoleEntry[]>(() => {
		const rows: ConsoleEntry[] = [];
		for (let i = start; i < Math.min(end, view.total()); i++) rows.push(view.get(i));
		return rows;
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
			{#each windowRows as row (row.uid)}
				<div
					class="row"
					class:err={row.level === 'error'}
					class:warn={row.level === 'warning'}
					data-testid="console-entry"
					data-node={row.node}
					data-stream={row.stream}
					data-level={row.level}
					use:measure={row.uid}
				>
					{#if !filterName}
						<button
							class="node"
							title={sourceLabel(row)}
							aria-label={sourceLabel(row)}
							onclick={(ev) => {
								ev.stopPropagation();
								if (row.node) focus(row.node);
							}}>{Array.from(sourceLabel(row))[0]}</button
						>
					{/if}
					<time title={new Date(row.ts).toISOString()}>{timeFormat.format(row.ts)}</time>
					<pre class="txt">{row.text}</pre>
					<div class="actions">
						{#if row.count > 1}
							<Badge data-testid="console-count" title="{row.count} occurrences"
								>×{row.count}</Badge
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
								copy(row.text, row.uid);
							}}><Icon name={copiedUid === row.uid ? 'check' : 'copy'} /></IconButton
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
		box-sizing: border-box;
	}
	.row.err {
		background: color-mix(in srgb, var(--danger) 9%, transparent);
		color: var(--danger);
	}
	.node, time, .actions { user-select: none; }
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
		width: 1ch;
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
