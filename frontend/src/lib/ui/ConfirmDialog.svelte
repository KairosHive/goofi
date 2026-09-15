<!-- ConfirmDialog — a modal that asks ONE question: the question, the prose under it, what the
     question is about when it needs showing, and the row of answers the caller supplies as buttons. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLAttributes } from 'svelte/elements';
	import Dialog from './Dialog.svelte';

	let {
		open,
		question,
		detail,
		onClose,
		body,
		children,
		...rest
	}: HTMLAttributes<HTMLDialogElement> & {
		open: boolean;
		question: string;
		/** What the answers mean, in a sentence. */
		detail: string;
		onClose: () => void;
		/** What the question is about, between the prose and the answers: a list to pick from. */
		body?: Snippet;
		/** The answers, as buttons. */
		children: Snippet;
	} = $props();
</script>

<Dialog {open} {onClose} {...rest}>
	<h2>{question}</h2>
	<p>{detail}</p>
	{@render body?.()}
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
