<!-- The node inspector of the editor side pane and the Inspector panel: identity, group tabs, one
     ParamField per shown param, metadata and the node's error. -->
<script lang="ts">
	import { PARAM_MODES, type ParamDescriptor, type ParamMode, type SourcePatch } from '$lib/api/types';
	import { getControl, type NodeInstanceInfo } from '$lib/api/control';
	import type { MenuItem } from 'panelty';
	import { ContextMenu, createLongPress } from 'panelty';
	import { graph, paramLive } from '$lib/stores/graph.svelte';
	import { ui } from '$lib/stores/ui.svelte';
	import { selection } from '$lib/stores/selection.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { isValidName } from '$lib/crdt/graphDoc';
	import { formatName } from '$lib/editor/categoryColor';
	import { bareName } from '$lib/editor/typeId';
	import { nodeHealth } from '$lib/editor/nodeHealth';
	import { isTextEditingTarget } from '$lib/ui/textEditing';
	import ParamField from './ParamField.svelte';
	import { isNumeric } from './controlKind';
	import { expressionFor, MODE_FACE, sourceForMode } from './paramSeed';
	import SubPatchInspector from '$lib/editor/SubPatchInspector.svelte';
	import MetadataPanel from '$lib/editor/MetadataPanel.svelte';
	import { matchParams, type ParamHit } from './paramSearch';
	import {
		admits,
		narrowing,
		paramKey,
		settleNonDefault,
		shown,
		SHOW_ALL,
		toggleSource,
		type Filters,
		type NonDefault
	} from './paramFilters';
	import {
		Bar,
		Tabs,
		Badge,
		Button,
		ConfirmDialog,
		Disclosure,
		EmptyState,
		Icon,
		IconButton,
		MODE_ATTRS,
		ScrollArea,
		Segmented
	} from '$lib/ui';

	let { node, onClose }: {
		node: NodeInstanceInfo | null;
		/** Renders a ✕ in the identity Bar; only the slide-in inspector supplies one. */
		onClose?: () => void;
	} = $props();

	const g = graph();
	const uiStore = ui();
	const live = paramLive();

	// A driven param's value is a READOUT, and the control plane paces one for health rather than
	// for reading: the socket is open only while this form is showing the node.
	$effect(() => (node ? live.watch(node.uid) : undefined));

	// Each RPC is fire-and-forget with a logged failure, so a rejection is never unhandled.
	function send(what: string, call: (uid: string) => Promise<unknown>): void {
		if (node) void call(node.uid).catch((e) => console.warn(`${what} failed`, e));
	}
	const setValue = (group: string, name: string, value: unknown): void =>
		send('update', (uid) => g.updateParam(uid, group, name, value));
	const setSource = (group: string, name: string, source: SourcePatch): void =>
		send('set source', (uid) => g.setSource(uid, group, name, source));

	// Keyed by uid, so switching nodes closes the editor while a live state update (which re-creates the
	// node object) leaves an open edit untouched.
	let editingUid = $state<string | null>(null);
	let nameDraft = $state('');
	const editingName = $derived(node != null && editingUid === node.uid);

	function startRename(): void {
		if (!node) return;
		nameDraft = node.name;
		editingUid = node.uid;
	}
	function commitRename(): void {
		// Escape/cancel nulls editingUid first, so the blur the unmounting input fires is a no-op here.
		const uid = editingUid;
		if (!uid || !node || node.uid !== uid) {
			editingUid = null;
			return;
		}
		const base = nameDraft.trim();
		// The manager refuses a name an expression could not read as an attribute; the field stays
		// open with the draft, marked bad, rather than throwing the user's typing away on a blur.
		if (!isValidName(base)) return;
		editingUid = null;
		void g.renameNode(uid, base).catch((e) => console.warn('rename failed', e));
	}
	function cancelRename(): void {
		editingUid = null;
	}
	function focusInput(el: HTMLInputElement): void {
		el.focus();
		el.select();
	}

	// Custom nodes can be saved from the patch or from the library.
	const savable = $derived.by(() => {
		const n = node;
		return n != null && (g.nodeTypes ?? []).some((t) => t.type === n.type && (t.source === 'patch' || t.source === 'custom'));
	});
	let saving = $state(false);
	let libraryName = $state('');
	// The type rides with the path: the inspector follows the selection, so by the time the dialog
	// is answered the node under it need not be the one the question was about.
	let replacing = $state<{ type: string; path: string } | null>(null);

	type LibraryEntry = { provenance?: string; path?: string; shadowed?: { provenance: string; path: string }[] };
	// A file the library already holds is a question before it is a save; an answered one is not
	// asked again.
	async function saveToLibrary(type: string, overwrite: boolean, name?: string): Promise<void> {
		saving = true;
		try {
			const r = overwrite || name ? null : await getControl().call<LibraryEntry>('library get', { type });
			const held = r?.provenance === 'custom' ? r.path : r?.shadowed?.find((s) => s.provenance === 'custom')?.path;
			if (held) {
				libraryName = bareName(type);
				replacing = { type, path: held };
			} else {
				await getControl().call('library save', { type, overwrite, ...(name ? { name } : {}) });
				replacing = null;
				notify().raise(`${bareName(type)} is in your library`);
			}
		} catch (e) {
			notify().failure('Save to library', e);
		} finally {
			saving = false;
		}
	}

	function replace(): void {
		const at = replacing;
		replacing = null;
		if (at) void saveToLibrary(at.type, true);
	}

	const formId = $props.id();
	$effect(() => {
		if (!replacing) return;
		const id = `${formId}-library`;
		uiStore.openEditor(id);
		return () => uiStore.closeEditor(id);
	});
	// A node dragged out of the editor onto a param row: the row is a target only where the node HAS
	// an output that param may follow, so the menu can never open on a link the manager would refuse.
	const dragged = $derived(uiStore.nodeDrag);
	$effect(() =>
		uiStore.onNodeDrop(formId, (uid, zone, at) => {
			const [group, name] = zone.split('/');
			const reference = g.referenceFor(uid, node?.params?.[group]?.[name]?.type ?? '');
			if (reference) menu = { x: at.x, y: at.y, items: linkItems(group, name, uid, reference) };
		})
	);
	let menu = $state<{ x: number; y: number; items: MenuItem[] } | null>(null);

	/** The two ways one dropped node can drive a param: followed, or read by an expression. */
	function linkItems(group: string, name: string, uid: string, reference: string): MenuItem[] {
		const expression = expressionFor(reference, Object.keys(g.nodeById(uid)?.output_slots ?? {}).length);
		return [
			{
				label: `Reference ${reference}`,
				icon: 'workflow',
				action: () => setSource(group, name, { reference })
			},
			{
				label: `Expression ${expression}`,
				icon: 'terminal',
				action: () => setSource(group, name, { expression })
			}
		];
	}

	function modulate(group: string, name: string, kind: 'lfo' | 'noi'): void {
		const freq = 0.01 + Math.random() * 0.19;
		setSource(group, name, { expression: `${kind}(freq=${freq})` });
	}

	/** The source the app's reference pick gives this param, or null where the param may not take it. */
	function pickedSource(d: ParamDescriptor): SourcePatch | null {
		const pick = selection().reference;
		if (!pick) return null;
		if ('variable' in pick) {
			return g.variables.some((v) => v.name === pick.variable) ? { expression: `variables.${pick.variable}` } : null;
		}
		const reference = pick.node === node?.uid ? null : g.referenceFor(pick.node, d.type, pick.slot);
		return reference ? { reference } : null;
	}

	/** A row's menu, from a right click or a held touch: LFO and Noise on a numeric row, and the pick. */
	function rowItems({ group, name, descriptor }: ParamHit): MenuItem[] {
		const items: MenuItem[] = isNumeric(descriptor)
			? [
					{ label: 'LFO', action: () => modulate(group, name, 'lfo') },
					{ label: 'Noise', action: () => modulate(group, name, 'noi') }
				]
			: [];
		const source = pickedSource(descriptor);
		if (source) {
			const label = `Reference selection: ${source.expression ?? source.reference}`;
			items.push({ label, icon: 'circle-dot', action: () => setSource(group, name, source) });
		}
		return items;
	}
	function rowMenu(x: number, y: number, hit: ParamHit): boolean {
		const items = rowItems(hit);
		if (items.length) menu = { x, y, items };
		return items.length > 0;
	}

	// One hold for the form: movement cancels it, and the click its release fires is swallowed.
	let pressed = false;
	let pressHit: ParamHit | undefined;
	const press = createLongPress((at) => {
		pressed = true;
		if (pressHit) rowMenu(at.clientX, at.clientY, pressHit);
	});
	function release(): void {
		activeParam = null;
		press.cancel();
	}

	/** Move between the main value controls in the displayed parameter rows. */
	function paramTabOrder(el: HTMLElement): { destroy(): void } {
		const keydown = (event: KeyboardEvent): void => {
			if (event.key !== 'Tab' || event.defaultPrevented || event.ctrlKey || event.metaKey || event.altKey) return;
			const controls = Array.from(el.querySelectorAll<HTMLElement>('[data-param-edit]'))
				.map((control) => control.matches('input, select, button')
					? control : control.querySelector<HTMLElement>('input, select, button'))
				.filter((control): control is HTMLElement =>
					control !== null && !control.matches(':disabled') && control.getClientRects().length > 0);
			const index = controls.indexOf(event.target as HTMLElement);
			if (index < 0) return;
			const next = controls[index + (event.shiftKey ? -1 : 1)];
			if (!next) return;
			event.preventDefault();
			next.focus();
		};
		el.addEventListener('keydown', keydown);
		return { destroy: () => el.removeEventListener('keydown', keydown) };
	}

	function onParamKey(event: KeyboardEvent): void {
		if (
			event.defaultPrevented || event.repeat || event.ctrlKey || event.metaKey ||
			event.altKey || event.shiftKey || event.isComposing || menu || isTextEditingTarget(event.target)
		) return;
		const key = event.key;
		if (!['l', 'n', 'r', 'c', 'e'].includes(key)) return;
		const row = document.querySelector<HTMLElement>(`[data-param-form="${formId}"]:hover`);
		const hit = rows.find((r) => paramKey(r.group, r.name) === row?.dataset.paramKey);
		if (!hit) return;
		const { group, name, descriptor: d } = hit;
		if (key === 'c' || key === 'e') {
			event.preventDefault();
			const mode = key === 'c' ? 'constant' : 'expression';
			if (d.mode !== mode) setSource(group, name, sourceForMode(d, mode));
			return;
		}
		if (!isNumeric(d)) return;
		if (key === 'r') {
			const min = d.type === 'int' ? Math.ceil(d.vmin) : d.vmin;
			const max = d.type === 'int' ? Math.floor(d.vmax) : d.vmax;
			if (!Number.isFinite(min) || !Number.isFinite(max) || min > max) return;
			event.preventDefault();
			const fraction = Math.random();
			const value = d.type === 'int'
				? Math.min(max, min + Math.floor(fraction * (max - min + 1)))
				: min * (1 - fraction) + max * fraction;
			setValue(group, name, value);
		} else {
			event.preventDefault();
			modulate(group, name, key === 'l' ? 'lfo' : 'noi');
		}
	}

	/** The row's drop-zone key, or null where this node cannot drive that param. */
	function dropZone(group: string, name: string, d: ParamDescriptor): string | null {
		if (!dragged || dragged === node?.uid) return null;
		return g.referenceFor(dragged, d.type) ? `${formId}#${paramKey(group, name)}` : null;
	}

	// A group whose every param is hidden has no tab.
	const groupNames = $derived(
		node ? Object.keys(node.params).filter((g) => Object.keys(node.params[g]).some((n) => shown(node.params, g, n))) : []
	);
	const health = $derived(nodeHealth(node));

	// A docstring leads with its one-line summary, so that line IS the row and the caret opens what
	// follows it. A node whose whole doc is that line has nothing to open, and gets no caret.
	const docHead = $derived((node?.doc ?? '').split('\n')[0].trim());
	const docRest = $derived((node?.doc ?? '').split('\n').slice(1).join('\n').trim());

	const tabItems = $derived(groupNames.map((name) => ({ id: name, label: name })));

	// DERIVED, so the right tab is in the first paint and there is no `.ui-tab` background transition to
	// animate; the effect then ADOPTS the fallback, which keeps the front group sticky.
	let frontGroup = $state<string | null>(null);
	const activeGroup = $derived.by<string | null>(() => {
		const valid = groupNames;
		if (valid.length === 0) return null;
		return frontGroup && valid.includes(frontGroup) ? frontGroup : valid[0];
	});
	$effect(() => {
		if (activeGroup !== frontGroup) frontGroup = activeGroup;
	});

	// One search box over every family, because a plugin's parameters are spread across as many tabs
	// as it has units and the one you want is rarely in the tab you are on.
	let query = $state('');
	const searching = $derived(query.trim().length > 0);

	// The filters narrow the GROUP the reader is in rather than dissolving the tabs, and they open
	// showing everything again whenever the selection moves.
	let filters = $state<Filters>(SHOW_ALL);
	let filtered = $state<string | null>(null);
	let activeParam = $state<string | null>(null);
	let nonDefault = $state<NonDefault>(new Map());
	$effect(() => {
		const uid = node?.uid ?? null;
		if (uid !== filtered) {
			filtered = uid;
			filters = SHOW_ALL;
			activeParam = null;
			nonDefault = new Map();
			menu = null; // its items act on the node it opened on
		}
	});
	$effect(() => {
		nonDefault = settleNonDefault(nonDefault, node?.params, node?.baseline, activeParam);
	});
	const changed = $derived(nonDefault.size);
	const narrowed = $derived(narrowing(filters));

	// A search spans every group, since the tabs cannot say where a plugin filed a knob; it still
	// finds only what the filters admit.
	const rows = $derived.by<ParamHit[]>(() => {
		const n = node;
		if (!n) return [];
		const all = searching
			? matchParams(n.params, query)
			: activeGroup ? Object.entries(n.params[activeGroup]).map(([name, descriptor]) => ({ group: activeGroup, name, descriptor })) : [];
		return all.filter((r) => admits(filters, r.descriptor, nonDefault, r.group, r.name) && shown(n.params, r.group, r.name));
	});
	/** The section a row sits in, and the slot of a list it is, so a heading opens each once. */
	const sectionOf = (r: ParamHit | undefined): string | null => r?.descriptor.role?.section ?? null;
	const slotOf = (r: ParamHit | undefined): string | null => {
		const role = r?.descriptor.role;
		return role?.as === 'member' && role.slot != null ? `${role.section}/${role.slot}` : null;
	};

	const FILTER_TITLE: Record<ParamMode, string> = {
		constant: 'the params holding a value set by hand',
		expression: 'the params Python drives',
		reference: "the params following a node's output slot"
	};
</script>

<svelte:window
	onkeydown={onParamKey}
	onpointerdown={(e) => {
		const row = (e.target as Element).closest<HTMLElement>('[data-param-key]');
		activeParam = row?.dataset.paramNode === node?.uid ? row?.dataset.paramKey ?? null : null;
		pressed = false;
		const hit = row?.dataset.paramForm === formId ? rows.find((r) => paramKey(r.group, r.name) === row.dataset.paramKey) : undefined;
		if (e.pointerType !== 'mouse' && hit && rowItems(hit).length) {
			pressHit = hit;
			press.start(e);
		}
	}}
	onpointermove={press.move}
	onpointerup={release}
	onpointercancel={release}
/>

<ScrollArea>
	<section class="param-form">
		{#if !node}
			<EmptyState data-testid="param-empty">
				{#snippet title()}No node selected{/snippet}
				{#snippet hint()}Select a node to edit its parameters.{/snippet}
			</EmptyState>
		{:else}
			<Bar class="pf-identity-bar">
				{#snippet start()}
					<div class="pf-identity">
						<div class="pf-title">
							{#if editingName}
								<!-- svelte-ignore a11y_autofocus -->
								<input
									{...MODE_ATTRS.search}
									class="pf-rename"
									class:bad={nameDraft.trim() !== '' && !isValidName(nameDraft.trim())}
									aria-label="Node name"
									value={nameDraft}
									oninput={(e) => (nameDraft = e.currentTarget.value)}
									onblur={commitRename}
									onkeydown={(e) => {
										if (e.key === 'Enter') commitRename();
										else if (e.key === 'Escape') cancelRename();
									}}
									data-testid="node-name-input"
									use:focusInput
								/>
							{:else}
								<button
									class="pf-name"
									title="Click to rename"
									onclick={startRename}
									data-testid="node-name">{node.name}</button
								>
							{/if}
						</div>
						<div class="pf-type">{formatName(bareName(node.type))}</div>
					</div>
				{/snippet}
				{#snippet end()}
					{#if savable}
						<IconButton
							variant="ghost"
							density="chrome"
							label="Save to custom library"
							title="Move this node's file into your private library, where every patch finds it"
							data-testid="save-to-library"
							disabled={saving}
							onclick={() => void saveToLibrary(node.type, false)}><Icon name="save" /></IconButton
						>
					{/if}
					{#if node.editor}
						<IconButton
							variant="ghost"
							density="chrome"
							label="Open plugin editor"
							title="Open this plugin's own editor, in a window on the machine goofi runs on"
							data-testid="inspector-editor"
							onclick={() => send('editor', (uid) => getControl().call('node editor', { node: uid }))}
							><Icon name="app-window" /></IconButton
						>
					{/if}
					<Badge
						tone={health.tone}
						class="pf-state"
						title={health.hint}
						data-testid="node-state"
					>
						{health.status}{#if health.runtime}<span class="pf-runtime" data-testid="node-runtime"
								>{health.runtime}</span
							>{/if}
					</Badge>
					{#if onClose}
						<IconButton
							variant="ghost"
							density="chrome"
							class="pf-close"
							label="Close inspector"
							title="Close the inspector"
							data-testid="inspector-close"
							onclick={onClose}><Icon name="x" /></IconButton
						>
					{/if}
				{/snippet}
			</Bar>

			{#if docRest}
				<Disclosure class="pf-docs">
					{#snippet summary()}
						<span data-testid="docs-toggle">{docHead}</span>
					{/snippet}
					{#snippet children()}
						<p class="pf-docstring" data-testid="docstring">{docRest}</p>
					{/snippet}
				</Disclosure>
			{:else if docHead}
				<p class="pf-doc-line" data-testid="docstring">{docHead}</p>
			{/if}

			{#if node.subpatch}
				<SubPatchInspector {node} />
			{:else}
				<div class="pf-tools" data-testid="param-filters">
					<!-- Native, not `TextInput`: this filters per keystroke and owns Escape. -->
					<input
						class="pf-search"
						{...MODE_ATTRS.search}
						bind:value={query}
						onkeydown={(e) => {
							if (e.key === 'Escape') query = '';
						}}
						placeholder={narrowed ? 'Search filtered…' : 'Search parameters…'}
						autocomplete="off"
						aria-label="Search parameters"
						data-testid="param-search"
					/>

					<div class="pf-nondefault">
						<Segmented
							value={filters.nonDefault ? 'non-default' : null}
							segments={[
								{
									id: 'non-default',
									label: '',
									icon: 'hand',
									count: changed,
									name: 'Non-default only',
									title:
										'Non-default only — the params on this node that no longer hold their default',
									testid: 'param-non-default-only'
								}
							]}
							onChange={() => (filters = { ...filters, nonDefault: !filters.nonDefault })}
						/>
						<!-- Clearing moves the default this counts from; it edits no param, so an expression
						     or a reference keeps driving and keeps answering the source strip. -->
						<IconButton
							variant="ghost"
							density="chrome"
							class="pf-clear"
							label="Clear non-default"
							title="Take every param's default from what this node holds now. Mappings keep working."
							data-testid="param-non-default-clear"
							disabled={changed === 0}
							onclick={() => send('clear non-default', (uid) => getControl().call('node baseline', { node: uid }))}
							><Icon name="eraser" /></IconButton
						>
					</div>
					<Segmented
						value={filters.sources}
						segments={PARAM_MODES.map((id) => ({
							id,
							...MODE_FACE[id],
							title: `${MODE_FACE[id].name} — ${FILTER_TITLE[id]}`,
							testid: `param-${id}-only`
						}))}
						onChange={(id) => (filters = toggleSource(filters, id as ParamMode))}
					/>
				</div>

				{#if tabItems.length > 0 && !searching}
					<Tabs
						items={tabItems}
						active={activeGroup ?? undefined}
						onSelect={(id) => (frontGroup = id)}
						data-testid="param-tabs"
						style="overflow-x: auto; flex-shrink: 0"
						tabProps={() => ({ style: 'min-width: 3rem; max-width: max-content' })}
					/>
				{/if}

				<!-- A tabpanel only when a tablist exists: an orphaned `tabpanel` role would have no owning tablist. -->
				<div
					class="pf-rows"
					use:paramTabOrder
					role={tabItems.length > 0 && !searching ? 'tabpanel' : undefined}
					aria-label={searching ? undefined : (activeGroup ?? undefined)}
					data-testid="param-rows"
					onclickcapture={(e) => {
						if (!pressed) return;
						pressed = false;
						e.preventDefault();
						e.stopPropagation();
					}}
				>
					{#if rows.length === 0}
						<div class="pf-empty-group" data-testid={searching ? 'param-no-matches' : 'param-empty-group'}>
							{#if searching}{narrowed ? 'No filtered parameters match.' : 'No parameters match.'}{:else if narrowed}Nothing in this group matches these filters — another group may still hold one.{:else}No parameters in this group.{/if}
						</div>
					{:else}
						{#each rows as { group, name: paramName, descriptor }, i (node.uid + '/' + group + '/' + paramName)}
							{@const role = descriptor.role}
							<!-- A line only between two shown rows, so a hidden section draws nothing. -->
							{#if !searching && i > 0 && rows[i - 1].descriptor.section !== descriptor.section}
								<hr class="pf-section" data-testid="param-section-break" />
							{/if}
							{#if !searching && role?.as === 'member' && role.slot == null && sectionOf(rows[i - 1]) !== role.section}
								<div class="pf-section-name" data-testid={`param-section-${role.section}`}>{role.section}</div>
							{/if}
							{#if !searching && role?.as === 'member' && role.slot != null && slotOf(rows[i - 1]) !== `${role.section}/${role.slot}`}
								<div class="pf-slot" data-testid={`param-slot-${role.section}-${role.slot}`}>{role.slot + 1}</div>
							{/if}
							<div
								class="pf-row"
								class:pf-count={role?.as === 'count'}
								role="group"
								aria-label={paramName}
								data-param-form={formId}
								data-param-node={node.uid}
								data-param-key={paramKey(group, paramName)}
								oncontextmenu={(e) => {
									if (!rowMenu(e.clientX, e.clientY, { group, name: paramName, descriptor })) return;
									e.preventDefault();
									e.stopPropagation();
								}}
							>
								{#if searching}
									<span class="pf-row-group" data-testid={`param-hit-group-${paramName}`}>{group}</span>
								{/if}
								{#if role?.as === 'count' && descriptor.type === 'int'}
									<!-- A list's count is its heading: the name, how many, and a step either way. -->
									<span class="pf-count-name" title={descriptor.doc ?? undefined}>{paramName}</span>
									<IconButton
										label={`One ${paramName} fewer`}
										variant="ghost"
										size="sm"
										disabled={descriptor.value <= descriptor.vmin || descriptor.mode !== 'constant'}
										onclick={() => setValue(group, paramName, descriptor.value - 1)}
										data-testid={`param-count-less-${paramName}`}
									>
										<Icon name="minus" />
									</IconButton>
									<span class="pf-count-value" data-testid={`param-count-${paramName}`}>{descriptor.value}</span>
									<IconButton
										label={`One ${paramName} more`}
										variant="ghost"
										size="sm"
										disabled={descriptor.value >= descriptor.vmax || descriptor.mode !== 'constant'}
										onclick={() => setValue(group, paramName, descriptor.value + 1)}
										data-testid={`param-count-more-${paramName}`}
									>
										<Icon name="plus" />
									</IconButton>
								{:else}
								<ParamField
									{paramName}
									label={role?.as === 'member' ? role.base : paramName}
									selfName={node?.name}
									{descriptor}
									dropZone={dropZone(group, paramName, descriptor)}
									data-testid={`param-field-${paramName}`}
									refreshing={node != null && g.isRefreshing(node.uid, group, paramName)}
									onCommit={(v) => setValue(group, paramName, v)}
									onPreview={(v) => node && g.previewParam(node.uid, group, paramName, v)}
									onSetSource={(source) => setSource(group, paramName, source)}
									onRefresh={() => send('refresh', (uid) => g.refreshParam(uid, group, paramName))}
									onPulse={() => send('pulse', (uid) => g.pulse(uid, group, paramName))}
								/>
								{/if}
							</div>
						{/each}
					{/if}
				</div>
			{/if}
			<MetadataPanel {node} />
			{#if node.error}
				<section class="node-error" data-testid="inspector-error">
					<div class="err-head">
						<header>Error</header>
						<Button
							variant="danger"
							size="sm"
							onclick={() => send('restart', (uid) => g.restartNode(uid))}
							title="Restart this node (respawn with the same params + links)"
							data-testid="inspector-restart">↻ Restart</Button
						>
					</div>
					<pre>{node.error}</pre>
				</section>
			{/if}
		{/if}
	</section>

	{#if menu}
		<ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => (menu = null)} />
	{/if}

	<ConfirmDialog
		class="nokey"
		open={!!replacing}
		question="Replace the node in your library?"
		detail={replacing
			? `Your library already holds ${bareName(replacing.type)}, at ${replacing.path}. Choose Overwrite to replace it, or enter another file name and choose Adapt name.`
			: ''}
		onClose={() => (replacing = null)}
		data-testid="library-replace-dialog"
	>
		<Button variant="danger" disabled={saving} data-testid="library-replace" onclick={replace}>Overwrite</Button>
		<!-- The button state follows each keystroke, before the field loses focus. -->
		<input aria-label="New file name" bind:value={libraryName} data-testid="library-name" />
		<Button disabled={saving || !libraryName.trim() || libraryName.trim() === bareName(replacing?.type ?? '')}
			data-testid="library-rename" onclick={() => {
				if (replacing) void saveToLibrary(replacing.type, false, libraryName.trim());
			}}>Adapt name</Button>
		<Button variant="ghost" onclick={() => (replacing = null)}>Cancel</Button>
	</ConfirmDialog>
</ScrollArea>

<style>
	.param-form {
		display: flex;
		flex-direction: column;
		min-width: 0;
	}
	.pf-identity {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		min-width: 0;
	}
	.pf-title {
		font-size: var(--fs-strong);
		font-weight: 600;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	/* Mono, stated after the `font: inherit` reset that would otherwise wipe it: the same identifier
	   the canvas paints on the node. */
	.pf-name,
	.pf-rename {
		font: inherit;
		font-family: var(--font-mono);
		color: var(--text);
	}
	.pf-name {
		background: none;
		border: none;
		padding: 0;
		cursor: text;
		border-radius: var(--radius-sm);
		/* The truncation is the BUTTON's own: `text-overflow` on `.pf-title` reaches the text in it,
		   never an overflowing child element, so a long name was cut mid-word with no ellipsis. */
		display: block;
		max-width: 100%;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.pf-name:hover {
		text-decoration: underline;
		text-decoration-style: dotted;
		text-underline-offset: 2px;
	}
	.pf-rename {
		width: 100%;
		font-size: var(--fs-strong);
		font-weight: 600;
		padding: var(--space-1) var(--space-2);
		background: var(--surface-2);
		border: 1px solid var(--accent);
		border-radius: var(--radius-sm);
	}
	.pf-rename.bad {
		color: var(--danger);
	}
	.pf-type {
		color: var(--text-muted);
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.pf-docstring {
		margin: 0;
		font-size: var(--fs-small);
		color: var(--text-dim);
		white-space: pre-wrap;
	}
	/* The docs row is chrome too, so it wears the header block's ground; what it opens keeps the
	   body's dark behind it. */
	.param-form :global(.pf-docs) {
		--disclosure-surface: var(--surface-2);
		--disclosure-hover: var(--surface-3);
	}
	/* A whole docstring that IS its summary line: the same row, with nothing to open. */
	.pf-doc-line {
		margin: 0;
		padding: var(--space-2) var(--space-3);
		background: var(--surface-2);
		color: var(--text);
		font-size: var(--fs-small);
		font-weight: 600;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	/* Anchored on `.param-form`, a real element of THIS template: `pf-identity-bar` is a class passed to
	   another component, and Svelte's scoping hash never reaches its markup. */
	.param-form :global(.pf-identity-bar) {
		/* The ✕ must never be squeezed into overflow past the pane's edge; the name is what ellipsizes. */
		--bar-end-min: max-content;
		/* Two lines tall by construction, so it takes back the padding a one-row strip has none of. */
		--bar-pad-y: var(--space-2);
	}
	.param-form :global(.pf-identity-bar .pf-close) {
		--panelty-icon-btn-size: 22px;
		color: var(--text-dim);
	}
	.param-form :global(.pf-identity-bar .pf-close:hover) {
		color: var(--text);
	}
	/* The runtime rides INSIDE the state pill rather than beside it: one pill is what the row has
	   space for, and a second would be the first thing a narrow pane drops. */
	.param-form :global(.pf-state .pf-runtime) {
		margin-inline-start: var(--space-2);
		padding-inline-start: var(--space-2);
		border-inline-start: 1px solid currentColor;
		opacity: 0.7;
	}
	/* Below this the row cannot seat name + state + ✕, and the overflow would walk the ✕ off screen; the
	   badge is what yields. Asked of the PANE, not the host panel. */
	@container (max-width: 180px) {
		.param-form :global(.pf-identity-bar .pf-state) {
			display: none;
		}
	}
	/* The SAME surface the active tab drops to, so the tab merges into the body with no seam line. */
	.pf-rows {
		display: flex;
		flex-direction: column;
		gap: var(--space-5);
		padding: var(--space-6);
		background: var(--surface-1);
	}
	.pf-empty-group {
		color: var(--text-muted);
		font-size: var(--fs-small);
		text-align: center;
		padding: var(--space-6) 0;
	}
	/* Chrome, not body: it wears the tab strip's own surface and horizontal padding, so the search,
	   the filters and the group tabs beneath them read as one header block. */
	.pf-tools {
		display: flex;
		align-items: center;
		gap: var(--space-4);
		padding: var(--space-2);
		background: var(--surface-2);
	}
	.pf-search {
		flex: 1 1 auto;
		min-width: 0;
		height: var(--chrome-control-h);
		padding: 0 var(--space-3);
		font-size: var(--fs-small);
	}
	.pf-search::placeholder {
		color: var(--text-muted);
	}
	/* The clear moves the default this one filter reads, so it rides against that toggle. */
	.pf-nondefault {
		display: flex;
		align-items: center;
	}
	.param-form :global(.pf-clear) {
		--panelty-icon-btn-size: var(--chrome-control-h);
		color: var(--text-muted);
	}
	.param-form :global(.pf-clear:hover:not(:disabled)) {
		color: var(--text);
	}
	.pf-section {
		margin: 0;
		border: none;
		border-top: 1px solid var(--border);
	}
	/* A heading hangs close to the rows it opens, under the list's own gap. */
	.pf-section-name,
	.pf-slot {
		color: var(--text-muted);
		font-size: var(--fs-small);
		margin-bottom: calc(var(--space-2) - var(--space-5));
	}
	.pf-slot {
		padding-left: var(--space-3);
	}
	.pf-count {
		flex-direction: row;
		align-items: center;
		gap: var(--space-2);
	}
	.pf-count-name {
		flex: 1 1 auto;
		font-size: var(--fs-small);
	}
	.pf-count-value {
		min-width: 2ch;
		text-align: center;
		font-variant-numeric: tabular-nums;
	}
	.pf-row {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		min-width: 0;
	}
	.pf-row-group {
		color: var(--text-muted);
		font-size: var(--fs-small);
	}
	/* With no hover the editable cue rests visible; the fields restate app.css's 16px coarse floor,
	   which their own size would outrank, so focusing one does not force-zoom iOS. */
	@media (hover: none) and (pointer: coarse) {
		.pf-name {
			text-decoration: underline;
			text-decoration-style: dotted;
			text-underline-offset: 2px;
		}
		.pf-rename,
		.pf-search {
			font-size: 16px;
		}
	}
	.node-error {
		padding: var(--space-6);
		border-top: 1px solid var(--border);
		background: var(--surface-1);
	}
	.err-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--space-5);
		margin-bottom: var(--space-3);
	}
	.node-error header {
		font-weight: 600;
		font-size: var(--fs-small);
		color: var(--danger);
	}
	/* Stated, not inherited: a bare <pre> takes app.css's `font: inherit`, which is the chrome face. */
	.node-error pre {
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		color: var(--text-dim);
		white-space: pre-wrap;
		word-break: break-word;
		margin: 0;
	}
</style>
