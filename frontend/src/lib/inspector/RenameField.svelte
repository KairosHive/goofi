<!-- A name that is a button until it is clicked, then a field: the one rename control an inspector's
     identity bar wears, for a node, a state or a machine. -->
<script lang="ts">
	import { MODE_ATTRS } from '$lib/ui';

	let {
		value,
		valid,
		onRename,
		label,
		testid
	}: {
		value: string;
		/** Whether a draft may be committed; a bad one stays open, marked, rather than thrown away. */
		valid: (name: string) => boolean;
		onRename: (name: string) => void;
		label: string;
		/** The button's testid; the field's is `${testid}-input`. */
		testid: string;
	} = $props();

	let editing = $state(false);
	let draft = $state('');

	function start(): void {
		draft = value;
		editing = true;
	}
	function commit(): void {
		// Escape nulls `editing` first, so the blur the unmounting input fires is a no-op here.
		if (!editing) return;
		const name = draft.trim();
		if (!valid(name)) return;
		editing = false;
		if (name !== value) onRename(name);
	}
	function focusInput(el: HTMLInputElement): void {
		el.focus();
		el.select();
	}
</script>

{#if editing}
	<!-- svelte-ignore a11y_autofocus -->
	<input
		{...MODE_ATTRS.search}
		class="rename"
		class:bad={draft.trim() !== '' && !valid(draft.trim())}
		aria-label={label}
		value={draft}
		oninput={(e) => (draft = e.currentTarget.value)}
		onblur={commit}
		onkeydown={(e) => {
			if (e.key === 'Enter') commit();
			else if (e.key === 'Escape') editing = false;
		}}
		data-testid={`${testid}-input`}
		use:focusInput
	/>
{:else}
	<button class="name" title="Click to rename" onclick={start} data-testid={testid}>{value}</button>
{/if}

<style>
	/* Mono, stated after the `font: inherit` reset that would otherwise wipe it: the same identifier
	   the canvas paints on the node. */
	.name,
	.rename {
		font: inherit;
		font-family: var(--font-mono);
		color: var(--text);
	}
	.name {
		background: none;
		border: none;
		padding: 0;
		cursor: text;
		border-radius: var(--radius-sm);
		/* The truncation is the BUTTON's own: `text-overflow` on the title reaches the text in it,
		   never an overflowing child element, so a long name was cut mid-word with no ellipsis. */
		display: block;
		max-width: 100%;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.name:hover {
		text-decoration: underline;
		text-decoration-style: dotted;
		text-underline-offset: 2px;
	}
	.rename {
		width: 100%;
		font-size: var(--fs-strong);
		font-weight: 600;
		padding: var(--space-1) var(--space-2);
		background: var(--surface-2);
		border: 1px solid var(--accent);
		border-radius: var(--radius-sm);
	}
	.rename.bad {
		color: var(--danger);
	}
	/* With no hover the editable cue rests visible; the field restates app.css's 16px coarse floor,
	   which its own size would outrank, so focusing it does not force-zoom iOS. */
	@media (hover: none) and (pointer: coarse) {
		.name {
			text-decoration: underline;
			text-decoration-style: dotted;
			text-underline-offset: 2px;
		}
		.rename {
			font-size: 16px;
		}
	}
</style>
