<script lang="ts">
	import { Icon } from '$lib/ui';

	let { name, body, inline }: { name: string; body: string; inline: string } = $props();
	const bodyId = $props.id();
	const firstLine = $derived(inline.split(/\r?\n/, 1)[0]);
	const multiline = $derived(firstLine.length < inline.length);
	let preview = $state<HTMLSpanElement>();
	let overflow = $state(false);
	let open = $state(false);
	const expandable = $derived(multiline || overflow);

	function measure(): void {
		if (preview) overflow = preview.scrollWidth > preview.clientWidth;
	}

	$effect(() => {
		firstLine;
		measure();
	});
	$effect(() => {
		if (!preview) return;
		const observer = new ResizeObserver(measure);
		observer.observe(preview);
		return () => observer.disconnect();
	});
	$effect(() => {
		if (!expandable) open = false;
	});
</script>

{#snippet row()}
	<span class="caret" class:available={expandable} class:open><Icon name="chevron-right" /></span>
	<span class="key">{name}</span>
	<!-- Keep the inline space measurable while the value is shown below. -->
	<span class="preview" class:concealed={open} aria-hidden={open} bind:this={preview}>{firstLine}{multiline ? '…' : ''}</span>
{/snippet}

<div class="meta-field" data-meta-key={name}>
	{#if expandable}
		<button class="row" type="button" aria-expanded={open} aria-controls={bodyId} onclick={() => (open = !open)}>
			{@render row()}
		</button>
	{:else}
		<div class="row">{@render row()}</div>
	{/if}
	{#if open}
		<div id={bodyId} class="value">{body}</div>
	{/if}
</div>

<style>
	.row {
		display: flex;
		align-items: baseline;
		gap: var(--space-5);
		width: 100%;
		padding: var(--space-2) var(--space-1);
		border: none;
		background: transparent;
		text-align: left;
		font-family: var(--font-mono);
		font-size: var(--fs-small);
		border-radius: var(--radius-sm);
	}
	button.row {
		cursor: pointer;
	}
	button.row:hover {
		background: var(--surface-2);
	}
	.caret {
		display: flex;
		align-self: center;
		flex: 0 0 auto;
		visibility: hidden;
		font-size: var(--fs-micro);
		color: var(--text-muted);
		transition: transform var(--dur-slow) var(--ease);
	}
	.caret.available {
		visibility: visible;
	}
	.caret.open {
		transform: rotate(90deg);
	}
	.key {
		flex: 0 1 auto;
		min-width: 0;
		overflow-wrap: anywhere;
		color: var(--text);
		font-weight: 600;
	}
	.preview {
		flex: 1 1 0;
		min-width: 0;
		color: var(--text-muted);
		font-size: var(--fs-micro);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: pre;
	}
	.preview.concealed {
		visibility: hidden;
	}
	.value {
		font-family: var(--font-mono);
		font-size: var(--fs-micro);
		color: var(--text-dim);
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		padding: var(--space-1) 0 var(--space-3) var(--space-7);
	}
</style>
