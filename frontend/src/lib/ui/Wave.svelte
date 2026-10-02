<!-- Wave — a run of dots a wave travels along: the "working" sign for a strip. It flows left to
     right; `--wave-flow: column` with `--wave-dx: 1; --wave-dy: 0` turns it down a vertical strip. -->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';

	let {
		halted = false,
		count = 12,
		class: klass = '',
		...rest
	}: HTMLAttributes<HTMLSpanElement> & {
		/** Stop the wave and paint it as a failure. */
		halted?: boolean;
		count?: number;
	} = $props();
</script>

<span {...rest} class={`ui-wave${halted ? ' halted' : ''} ${klass}`.trim()} aria-hidden="true">
	{#each { length: count } as _, i (i)}<i style="--i: {i}"></i>{/each}
</span>

<style>
	.ui-wave {
		display: flex;
		flex-direction: var(--wave-flow, row);
		align-items: center;
		justify-content: center;
		gap: var(--space-2);
		flex: 0 0 auto;
		/* The strip is wide enough for a dot to sway without leaving it. */
		min-width: 2rem;
		min-height: 2rem;
	}
	.ui-wave i {
		width: 0.5rem;
		height: 0.5rem;
		border-radius: 50%;
		background: var(--accent);
		animation: ui-wave 1.6s ease-in-out infinite;
		/* A quarter cycle per dot: one full wave spans four of them. */
		animation-delay: calc(var(--i) * -0.25s);
	}
	.ui-wave.halted i {
		animation: none;
		background: var(--danger);
	}
	@keyframes ui-wave {
		0%,
		100% {
			translate: calc(var(--wave-dx, 0) * 0.5rem) calc(var(--wave-dy, 1) * 0.5rem);
			scale: 0.7;
			opacity: 0.3;
		}
		50% {
			translate: calc(var(--wave-dx, 0) * -0.5rem) calc(var(--wave-dy, 1) * -0.5rem);
			scale: 1;
			opacity: 1;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.ui-wave i {
			animation: none;
			opacity: 0.6;
		}
	}
</style>
