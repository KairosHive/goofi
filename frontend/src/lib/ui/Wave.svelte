<!-- Wave — a run of dots a wave travels along: the "working" sign for a strip. It flows left to
     right; `--wave-flow: column` with `--wave-dx: 1; --wave-dy: 0` turns it down a vertical strip. -->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';

	let {
		halted = false,
		class: klass = '',
		...rest
	}: HTMLAttributes<HTMLSpanElement> & {
		/** Stop the wave and paint it as a failure. */
		halted?: boolean;
	} = $props();

	/** The share of the strip's length the run takes, so it never reaches the edges. */
	const FILL = 0.9;
	let el = $state<HTMLSpanElement | null>(null);
	let count = $state(4);

	// As many dots as fit in nine tenths of the strip, re-counted whenever the strip resizes.
	$effect(() => {
		if (!el) return;
		const host = el;
		const fit = () => {
			const cs = getComputedStyle(host);
			const column = cs.flexDirection === 'column';
			const length = column ? host.clientHeight : host.clientWidth;
			const gap = parseFloat(column ? cs.rowGap : cs.columnGap) || 0;
			const dot = host.firstElementChild?.clientWidth || 8;
			count = Math.max(1, Math.floor((length * FILL + gap) / (dot + gap)));
		};
		const ro = new ResizeObserver(fit);
		ro.observe(host);
		return () => ro.disconnect();
	});
</script>

<span bind:this={el} {...rest} class={`ui-wave${halted ? ' halted' : ''} ${klass}`.trim()} aria-hidden="true">
	{#each { length: count } as _, i (i)}<i style={`--i: ${i}; --k: ${0.3 + (0.7 * Math.min(i, count - 1 - i, 3)) / 3}`}></i>{/each}
</span>

<style>
	.ui-wave {
		display: flex;
		flex-direction: var(--wave-flow, row);
		align-items: center;
		justify-content: center;
		gap: var(--space-8);
		flex: 0 0 auto;
		align-self: stretch;
		/* The strip is wide enough for a dot to sway without leaving it. */
		min-width: 2rem;
		min-height: 2rem;
	}
	.ui-wave i {
		flex: none;
		width: 0.5rem;
		height: 0.5rem;
		border-radius: 50%;
		background: var(--accent);
		animation: ui-wave 1.6s ease-in-out infinite;
		/* A quarter cycle per dot: one full wave spans four of them. */
		animation-delay: calc(var(--i) * -0.25s);
		/* `--k` tapers the outer three dots at each end, so the run fades out rather than stops. */
		--k: 1;
	}
	.ui-wave.halted i {
		animation: none;
		background: var(--danger);
	}
	@keyframes ui-wave {
		0%,
		100% {
			translate: calc(var(--wave-dx, 0) * 0.5rem) calc(var(--wave-dy, 1) * 0.5rem);
			scale: calc(0.7 * var(--k));
			opacity: calc(0.3 * var(--k));
		}
		50% {
			translate: calc(var(--wave-dx, 0) * -0.5rem) calc(var(--wave-dy, 1) * -0.5rem);
			scale: var(--k);
			opacity: var(--k);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.ui-wave i {
			animation: none;
			opacity: 0.6;
		}
	}
</style>
