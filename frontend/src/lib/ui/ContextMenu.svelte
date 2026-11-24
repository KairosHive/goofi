<!-- ContextMenu — panelty's menu over a `Catcher`: the press that closes it does nothing else. -->
<script lang="ts">
	import type { ComponentProps } from 'svelte';
	import { ContextMenu } from 'panelty';
	import Catcher from './Catcher.svelte';

	let props: ComponentProps<typeof ContextMenu> = $props();
</script>

<Catcher />
<ContextMenu {...props} />
