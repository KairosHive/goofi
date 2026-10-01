<script lang="ts">
	import { loadPlugins } from '$lib/plugins/runtime.svelte';
	import TopBar from '$lib/editor/TopBar.svelte';
	import FsBrowser from '$lib/fs/FsBrowser.svelte';
	import { patchName, patchStem, uploadPatch } from '$lib/api/patchFile';
	import ErrorPanel from '$lib/editor/ErrorPanel.svelte';
	import Toast from '$lib/app/Toast.svelte';
	import AgentClose from '$lib/app/AgentClose.svelte';
	import RecoverDialog from '$lib/app/RecoverDialog.svelte';
	import TitleTip from '$lib/app/TitleTip.svelte';
	import PresenceOverlay from '$lib/app/PresenceOverlay.svelte';
	import { presence } from '$lib/stores/presence.svelte';
	import { Tabs as WorkspaceTabs } from 'panelty';
	import { Panels as WorkspaceView } from 'panelty';
	import { registerAppPanels } from '$lib/panels/register';
	import { editorFor } from '$lib/panels/editorCommands';
	import { workspace } from 'panelty';
	import { layoutHost } from '$lib/stores/layoutHost';
	import { DEFAULT_PANEL_TYPE } from '$lib/api/vocab';
	import { graph } from '$lib/stores/graph.svelte';
	import { ui } from '$lib/stores/ui.svelte';
	import { history } from '$lib/stores/history.svelte';
	import { notify } from '$lib/stores/notify.svelte';
	import { shellKeyAction } from '$lib/app/shellKeys';
	import { isTextEditingTarget } from '$lib/ui';
	import { exposeAgentApi } from '$lib/agent';
	import { Button } from '$lib/ui';
	import { getControl } from '$lib/api/control';
	import { onMount } from 'svelte';

	let protocolMismatch = $state(false);

	// Before any panel renders; the pre-sync frame is the manager's own first-mint spelling, so the
	// editor mounts once.
	registerAppPanels();
	workspace().configureHost(layoutHost(), [
		{ id: 'tab-1', name: 'Tab 1', root: { kind: 'panel', id: 'panel-2', panelType: DEFAULT_PANEL_TYPE } }
	]);
	exposeAgentApi();

	const g = graph();
	const ws = workspace();

	function focusError(uid: string): void {
		editorFor(ws.activePanelId)?.focusNode(uid);
	}

	let fsMode = $state<null | 'save' | 'load'>(null);

	function triggerSave(): void {
		const path = g.savePath;
		if (path) {
			// The one save with no dialog in front of it, so a rejection has no other surface.
			void g.save(path).catch((e) => notify().failure('Save', e));
		} else {
			fsMode = 'save';
		}
	}

	function triggerLoad(): void {
		fsMode = 'load';
	}

	// The dialog stays up while the operation runs, showing its progress, and closes once it
	// succeeds; a failure stays in the dialog with its log, so the rejection reaches it.
	async function onFsPick(pickedPath: string, overwrite = false): Promise<void> {
		if (fsMode === 'save') await g.save(pickedPath, overwrite);
		else if (fsMode === 'load') await getControl().call('session load', { path: pickedPath });
		fsMode = null;
	}

	/** Upload a `.gfi` from the user's own machine, for what the backend's browser cannot reach. */
	async function onFsFilePick(file: File): Promise<void> {
		await uploadPatch(file);
		fsMode = null;
	}

	function onKeydown(e: KeyboardEvent): void {
		// The DOM answers for a native modal: it closes itself on Escape, and marks no event.
		const modal = ui().modalOpen || !!(e.target as HTMLElement | null)?.closest?.('dialog[open]');
		const key = e.key.toLowerCase();
		if ((e.ctrlKey || e.metaKey) && (key === 's' || key === 'o')) {
			// Claimed even when standing down: that is what keeps Chrome's own Save/Open off screen.
			e.preventDefault();
			if (modal) return;
			if (key === 's') triggerSave();
			else triggerLoad();
			return;
		}
		const action = shellKeyAction(e, modal || isTextEditingTarget(e.target), ws.maximizedPanelId !== null);
		if (!action) return;
		e.preventDefault();
		if (action === 'exit-maximize') ws.exitMaximize();
		else void history()[action]();
	}

	function onPointerDown(e: PointerEvent): void {
		if (!e.isPrimary || e.button !== 0 || e.shiftKey) return;
		// Drag handlers can cancel the browser action that clears the previous text selection.
		// Clear it before those handlers run; the browser can then start a new text selection.
		const selection = window.getSelection();
		if (selection?.toString()) selection.removeAllRanges();
	}

	// The viewpoint is this client's alone: stored, never converged, and it cannot dirty the patch.
	let pushTimer: ReturnType<typeof setTimeout> | null = null;
	const greeted = $derived(g.sessionEpoch > 0);
	function flushViewpoint(): void {
		if (!pushTimer) return;
		clearTimeout(pushTimer);
		pushTimer = null;
		void g.setViewpoint(ws.viewpoint());
	}

	$effect(() => {
		void ws.viewpointEpoch; // track: bumped by every viewpoint change
		// A fresh client must not overwrite the stored viewpoint with its own default.
		if (!greeted) return;
		if (pushTimer) clearTimeout(pushTimer);
		pushTimer = setTimeout(flushViewpoint, 400);
	});

	onMount(() => {
		let started = false;
		let disposed = false;
		let cleanup: (() => void) | undefined;
		const offPlugins = getControl().onConnect((connected) => {
			if (!connected || started) return;
			started = true;
			void loadPlugins().then((dispose) => {
				if (disposed) dispose();
				else cleanup = dispose;
			}).catch((error) => notify().failure('Plugins', error));
		});
		window.addEventListener('pointerdown', onPointerDown, true);
		window.addEventListener('keydown', onKeydown);
		// No leave-page prompt: the manager holds the state; only the debounced viewpoint is flushed.
		window.addEventListener('beforeunload', flushViewpoint);
		const offProto = getControl().onProtocolMismatch(() => (protocolMismatch = true));
		const leave = presence().start();
		return () => {
			leave();
			disposed = true;
			offPlugins();
			cleanup?.();
			window.removeEventListener('pointerdown', onPointerDown, true);
			window.removeEventListener('keydown', onKeydown);
			window.removeEventListener('beforeunload', flushViewpoint);
			offProto();
			if (pushTimer) clearTimeout(pushTimer);
		};
	});
</script>

<svelte:head>
	<title>{g.unsavedChanges ? '● ' : ''}{g.savePath ? patchName(g.savePath) : 'goofi'}</title
	>
</svelte:head>

<div class="app-root">
	{#if protocolMismatch}
		<div class="proto-banner" role="alert">
			<span>This page is out of date with the backend. Reload to continue.</span>
			<Button size="sm" onclick={() => location.reload()}>Reload</Button>
		</div>
	{/if}
	<TopBar onSave={triggerSave} onSaveAs={() => (fsMode = 'save')} onLoad={triggerLoad}>
		{#snippet tabs()}
			<WorkspaceTabs />
		{/snippet}
	</TopBar>
	<PresenceOverlay />
	<div class="main">
		<WorkspaceView />
		<ErrorPanel onFocus={focusError} />
	</div>
	{#if fsMode}
		<FsBrowser
			mode={fsMode}
			suggestedName={g.savePath ? patchStem(g.savePath) : ''}
			onPick={onFsPick}
			onFilePick={onFsFilePick}
			onClose={() => (fsMode = null)}
		/>
	{/if}
	<Toast />
	<!-- Shell chrome, not a panel: it asks about an INSTANCE, and asking must not dirty the patch. -->
	<AgentClose />
	<RecoverDialog />
	<!-- One layer, mounted once, so every `title=` below is reachable without hover. -->
	<TitleTip />
	{#if g.disconnected}
		<!-- An overlay, not a ring on the shell: panels make their own stacking contexts and would
		     paint over one. -->
		<div class="net-frame" data-testid="net-frame" aria-hidden="true"></div>
	{/if}
</div>

<style>
	.app-root {
		position: fixed;
		inset: 0;
		display: flex;
		flex-direction: column;
		min-width: 0;
		min-height: 0;
		/* On the shell, not `body`: this box is fixed, so an ancestor's padding cannot reach it. */
		padding: var(--safe-top) var(--safe-right) var(--safe-bottom) var(--safe-left);
	}
	/* An `inset` shadow because an outline would land outside the fixed box and clip to nothing. */
	.net-frame {
		position: fixed;
		inset: 0;
		pointer-events: none;
		box-shadow: inset 0 0 0 3px var(--warning);
		z-index: var(--z-toast);
	}
	.main {
		position: relative;
		flex: 1;
		min-width: 0;
		min-height: 0;
		display: flex;
	}
	.proto-banner {
		display: flex;
		align-items: center;
		justify-content: center;
		gap: var(--space-6);
		padding: var(--space-3) var(--space-6);
		background: var(--danger);
		color: var(--on-danger);
		font-size: var(--fs-small);
	}
</style>
