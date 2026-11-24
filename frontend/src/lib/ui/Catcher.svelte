<!-- Catcher — the layer under an open overlay (a popover, a pick list, a menu) that takes the
     press which closes it, so that press closes the overlay and does nothing to what lies under
     it. The overlay's own window listener closes it: this layer only takes the event. -->
<script lang="ts">
	import { portal } from 'panelty';
</script>

<div class="ui-catcher" use:portal></div>

<style>
	.ui-catcher {
		position: fixed;
		inset: 0;
		z-index: calc(var(--z-menu) - 1);
	}
</style>
