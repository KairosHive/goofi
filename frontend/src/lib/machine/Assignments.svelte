<!-- Local attribute assignments, shared by state entry/exit and transition actions. -->
<script lang="ts">
	import type { Attribute, Literal } from '$lib/api/generated';
	import ParamField from '$lib/inspector/ParamField.svelte';
	import { Button, Field, Icon, IconButton } from '$lib/ui';
	import { descriptorFor, literalOf } from './attributes';

	let { attributes, values, viewKey, onCommit, onPreview, testid = 'assignment' }: {
		attributes: Record<string, Attribute>; values: Record<string, Literal>; viewKey: string;
		onCommit: (patch: Record<string, Literal | null>) => void;
		onPreview?: (patch: Record<string, Literal>) => void; testid?: string;
	} = $props();
</script>

{#if Object.keys(attributes).length === 0}
	<div class="empty">Select the machine to add attributes.</div>
{/if}
{#each Object.entries(attributes) as [attr, a] (attr)}
	{#if attr in values}
		<div class="value" data-testid={`${testid}-value-${attr}`}>
			<ParamField paramName={attr} descriptor={descriptorFor(a, values[attr])} literal viewKey={`${viewKey}/${attr}`}
				onCommit={(v) => onCommit({ [attr]: literalOf(v) })}
				onPreview={onPreview ? (v) => onPreview?.({ [attr]: literalOf(v) }) : undefined} />
			<IconButton variant="ghost" size="sm" label={`Clear ${attr}`} title="Keep the held value" data-testid={`${testid}-clear`}
				onclick={() => onCommit({ [attr]: null })}><Icon name="minus" /></IconButton>
		</div>
	{:else}
		<div class="fields">
			<Field label={attr} row doc="Unset: keep the held value">
				<Button variant="ghost" size="sm" data-testid={`${testid}-unset-${attr}`} onclick={() => onCommit({ [attr]: a.default })}>set here</Button>
			</Field>
		</div>
	{/if}
{/each}

<style>
	.value { display: flex; align-items: flex-start; gap: var(--space-2); }
	.value > :global(.pf-param) { flex: 1 1 auto; min-width: 0; margin-inline: 0; }
</style>
