<!-- A machine's expression under its label: a transition's condition or a state's decision, both
     read against the machine's own scope — `variables.*`, `me`, `st()` and `t` — with the one
     editor a param's expression uses, and a line under it that says what the expression is for. -->
<script lang="ts">
	import ExprEditor from '$lib/inspector/expr/ExprEditor.svelte';

	let {
		machine,
		label,
		hint,
		value,
		placeholder = '',
		testid,
		onCommit
	}: {
		machine: string;
		label: string;
		/** What the expression answers, in a line; the label's tooltip says the same. */
		hint: string;
		value: string;
		placeholder?: string;
		testid: string;
		onCommit: (value: string) => void;
	} = $props();
</script>

<div class="expr-row" title={hint}>
	<span class="expr-label">{label}</span>
	<ExprEditor {machine} {value} {placeholder} {testid} {label} {onCommit} />
	<span class="expr-hint">{hint}</span>
</div>

<style>
	.expr-row {
		display: flex;
		flex-direction: column;
		gap: var(--space-2);
		min-width: 0;
		font-size: var(--fs-small);
	}
	/* The label a Field wears, so the row reads as one more of the inspector's. */
	.expr-label {
		font-weight: 600;
		letter-spacing: 0.01em;
	}
	.expr-hint {
		color: var(--text-muted);
		font-size: var(--fs-micro);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
</style>
