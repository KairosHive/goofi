<!-- PaintPad — a canvas that paints a drawing's byte code. A hand stroke paints locally while the
     pointer is down; on pointer up it goes out as ONE `control paint` op, the CLI's own door. -->
<script lang="ts">
	import { Button } from 'panelty';
	import { SPAN, decode, paint, strokeText } from '$lib/api/drawing';

	let {
		value,
		onStroke,
		disabled = false
	}: {
		value: string;
		/** Append ops, in the text form `control paint` takes. */
		onStroke: (ops: string) => void;
		disabled?: boolean;
	} = $props();

	/** The bitmap's own size: resizing the widget rescales the picture rather than cropping it. */
	const SIZE = 512;

	let canvas = $state<HTMLCanvasElement | null>(null);
	let hex = $state('#4aa3ff');
	let size = $state(24);
	let soft = $state(0);
	let erasing = $state(false);
	/** The stroke under the pointer: `[x, y, ms since the last point]`, and when its last point came. */
	let points: [number, number, number][] = [];
	let lastAt = 0;
	/** When the previous stroke ended, so the next one's time counts from it. */
	let endedAt = 0;
	let startAt = 0;

	$effect(() => {
		const ctx = canvas?.getContext('2d');
		if (ctx) paint(ctx, decode(value), SIZE);
	});

	function at(e: PointerEvent): [number, number] | null {
		if (!canvas) return null;
		const box = canvas.getBoundingClientRect();
		return [((e.clientX - box.left) / box.width) * SPAN, ((e.clientY - box.top) / box.height) * SPAN];
	}

	const ink = () => (erasing ? 'erase' : hex);

	/** The newest piece of the stroke under the pointer, painted locally until the drawing echoes. */
	function local(from: [number, number], to: [number, number]): void {
		const ctx = canvas?.getContext('2d');
		if (!ctx) return;
		const k = SIZE / SPAN;
		ctx.save();
		ctx.globalCompositeOperation = erasing ? 'destination-out' : 'source-over';
		ctx.strokeStyle = erasing ? '#000' : hex;
		ctx.lineWidth = Math.max(size * k, 0.5);
		ctx.lineCap = 'round';
		ctx.lineJoin = 'round';
		if (soft * k >= 0.5) ctx.filter = `blur(${soft * k}px)`;
		ctx.beginPath();
		ctx.moveTo(from[0] * k, from[1] * k);
		ctx.lineTo(to[0] * k, to[1] * k);
		ctx.stroke();
		ctx.restore();
	}

	function down(e: PointerEvent): void {
		if (disabled) return;
		const p = at(e);
		if (!p) return;
		startAt = lastAt = performance.now();
		points = [[p[0], p[1], 0]];
		(e.currentTarget as HTMLCanvasElement).setPointerCapture(e.pointerId);
		local(p, p);
		e.preventDefault();
	}
	function move(e: PointerEvent): void {
		const last = points.at(-1);
		const p = at(e);
		if (!last || !p) return;
		const now = performance.now();
		points.push([p[0], p[1], now - lastAt]);
		lastAt = now;
		local([last[0], last[1]], p);
	}
	function up(): void {
		if (!points.length) return;
		onStroke(strokeText(ink(), size, soft, points, endedAt ? startAt - endedAt : 0));
		points = [];
		endedAt = lastAt;
	}
	function clear(): void {
		if (!disabled) onStroke('clear');
	}
</script>

<div class="pad" data-testid="paint-pad">
	<div class="tools">
		<label class="swatch" title="Pick the ink" style={`--ink: ${hex}`}>
			<input
				type="color"
				value={hex}
				{disabled}
				data-testid="paint-colour"
				oninput={(e) => (hex = (e.currentTarget as HTMLInputElement).value)}
			/>
		</label>
		<label class="dial" title={`Brush ${size} across`}>
			<span>size</span>
			<input
				type="range"
				min="2"
				max="190"
				step="1"
				bind:value={size}
				{disabled}
				data-testid="paint-size"
			/>
		</label>
		<label class="dial" title={`Softness ${soft}`}>
			<span>soft</span>
			<input
				type="range"
				min="0"
				max="64"
				step="1"
				bind:value={soft}
				{disabled}
				data-testid="paint-soft"
			/>
		</label>
		<Button
			size="sm"
			variant={erasing ? 'primary' : 'ghost'}
			title="Paint transparency instead of colour"
			{disabled}
			data-testid="paint-eraser"
			onclick={() => (erasing = !erasing)}>erase</Button
		>
		<Button
			size="sm"
			variant="ghost"
			title="Clear the drawing"
			{disabled}
			data-testid="paint-clear"
			onclick={clear}>clear</Button
		>
	</div>
	<canvas
		bind:this={canvas}
		class="sheet"
		width={SIZE}
		height={SIZE}
		data-testid="paint-canvas"
		onpointerdown={down}
		onpointermove={move}
		onpointerup={up}
		onpointercancel={up}
	></canvas>
</div>

<style>
	.pad {
		display: flex;
		flex-direction: column;
		gap: var(--space-1);
		width: 100%;
		height: 100%;
		min-height: 0;
	}
	.tools {
		display: flex;
		align-items: center;
		gap: var(--space-1);
		flex-wrap: wrap;
	}
	.swatch {
		width: 22px;
		height: 22px;
		border-radius: var(--radius-sm);
		background: var(--ink);
		border: 1px solid var(--border);
		overflow: hidden;
		cursor: pointer;
		flex: none;
	}
	.swatch input {
		opacity: 0;
		width: 100%;
		height: 100%;
		cursor: pointer;
	}
	.dial {
		display: flex;
		align-items: center;
		gap: var(--space-1);
		min-width: 0;
		flex: 1 1 44px;
		color: var(--text-dim);
	}
	.dial span {
		font-size: var(--fs-micro);
		letter-spacing: 0.02em;
		flex: none;
	}
	.dial input {
		width: 100%;
		min-width: 32px;
		accent-color: var(--accent);
	}
	.sheet {
		flex: 1 1 auto;
		width: 100%;
		min-height: 0;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		background: var(--surface-1);
		cursor: crosshair;
		touch-action: none;
	}
</style>
