<!-- TextInput — a text control: `value` in, `onChange` out, committed on blur. One line commits on
     Enter; `multiline` keeps every key but Escape, which gives the board its keys back. -->
<script lang="ts">
	import type { HTMLInputAttributes, HTMLTextareaAttributes } from 'svelte/elements';
	import { useLiveValue } from './liveValue.svelte';
	import { claimFieldControlId } from './field';
	import { MODE_ATTRS, type InputModeVariant } from './inputMode';

	let {
		value,
		onChange,
		inputmode = 'text',
		multiline = false,
		class: klass = '',
		onkeydown,
		...rest
	}: Omit<HTMLInputAttributes, 'value' | 'type' | 'inputmode' | 'oninput' | 'onchange'> & {
		value: string;
		onChange: (v: string) => void;
		inputmode?: InputModeVariant;
		/** A `<textarea>`, where Enter is a newline and never a commit. */
		multiline?: boolean;
	} = $props();

	const ownId = $props.id();
	const fieldId = claimFieldControlId(ownId);
	const live = useLiveValue<string>(
		() => value,
		(v) => onChange(v)
	);
	const modeAttrs = $derived(MODE_ATTRS[inputmode]);
	const blur = () => {
		live.commit(live.value);
		live.end();
	};
	const typed = (e: Event) => live.input((e.currentTarget as HTMLInputElement | HTMLTextAreaElement).value);
</script>

{#if multiline}
	<textarea
		{...rest as HTMLTextareaAttributes}
		id={fieldId}
		class={`ui-textarea ${klass}`.trim()}
		spellcheck="false"
		autocomplete="off"
		value={live.value}
		onfocus={() => live.begin()}
		onblur={blur}
		onkeydown={(e) => {
			onkeydown?.(e as never);
			if (e.key === 'Escape') e.currentTarget.blur();
			else e.stopPropagation();
		}}
		oninput={typed}
	></textarea>
{:else}
	<input
		{...rest}
		{...modeAttrs}
		id={fieldId}
		type="text"
		class={`ui-text ${klass}`.trim()}
		value={live.value}
		onfocus={() => live.begin()}
		onblur={blur}
		onkeydown={(e) => {
			onkeydown?.(e);
			if (e.key === 'Enter') e.currentTarget.blur();
		}}
		oninput={typed}
	/>
{/if}

<style>
	.ui-text,
	.ui-textarea {
		flex: 1 1 auto;
		min-width: 0;
		width: 100%;
		color: var(--text);
	}
	.ui-textarea {
		height: 100%;
		min-height: 0;
		resize: none;
		line-height: 1.45;
		padding: var(--space-1) var(--space-2);
		overflow: auto;
	}
</style>
