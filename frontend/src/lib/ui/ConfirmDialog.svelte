<!-- ConfirmDialog — a modal that asks ONE question: the question, the prose under it, and the row
     of answers the caller supplies as buttons. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLAttributes } from 'svelte/elements';
	import Dialog from './Dialog.svelte';

	let {
		open,
		question,
		detail,
		onClose,
		children,
		...rest
	}: HTMLAttributes<HTMLDialogElement> & {
		open: boolean;
		question: string;
		/** What the answers mean, in a sentence. */
		detail: string;
		onClose: () => void;
		/** The answers, as buttons. */
		children: Snippet;
	} = $props();
</script>

<Dialog {open} {onClose} {...rest}>
	<h2>{question}</h2>
	<p>{detail}</p>
	<div class="choices">{@render children()}</div>
</Dialog>

<style>
	h2 {
		margin: 0 0 var(--space-4);
		font-size: var(--fs-body);
		font-weight: 600;
	}
	p {
		margin: 0;
		color: var(--text-dim);
		font-size: var(--fs-small);
	}
	.choices {
		display: flex;
		flex-wrap: wrap;
		gap: var(--space-3);
		justify-content: flex-end;
		margin-top: var(--space-6);
	}
</style>
