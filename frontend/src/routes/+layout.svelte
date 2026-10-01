<script lang="ts">
	// Order is load-bearing: panelty declares the `-default` token tier, app.css overrides it.
	import 'panelty/tokens.css';
	import '../app.css';
	import '@xyflow/svelte/dist/style.css';
	import { onMount } from 'svelte';
	import { trackKeyboardInset } from '$lib/stores/device';

	let { children } = $props();

	onMount(trackKeyboardInset);
</script>

{@render children?.()}
