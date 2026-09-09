<!-- DrawPad — a canvas you paint on, whose value is a `data:image/png;base64,…` URL. That is a
     STRING like any other, so a drawing crosses the wire, saves into the patch and is read by an
     expression through machinery that was already there; nothing new had to learn about images.

     The stroke is committed on pointer UP, never per move: a data URL runs to tens of kilobytes,
     and one per pointer event would put megabytes a second through the document.

     ONE colour control: the swatch, which opens the platform's own picker. A hue wheel and a
     brightness slider stood beside it and were three doors onto one colour — the wheel could not
     say what the picker could, so the two disagreed on every grey. -->
<script lang="ts">
	import { Button } from '$lib/ui';
	import type { Mark } from '$lib/api/control';

	let {
		value,
		onChange,
		pending = null,
		disabled = false
	}: {
		value: string;
		onChange: (v: string) => void;
		/** Strokes a turtle script asked for, newest batch last. */
		pending?: { id: number; marks: Mark[] } | null;
		disabled?: boolean;
	} = $props();

	/** The bitmap's own size, independent of the widget's box: resizing the widget rescales the
	    picture rather than cropping it. */
	const SIZE = 512;
	/** The square everything OUTSIDE the bitmap is said in — a pointer's position, a brush across,
	    a turtle step. One space, so `width 40` from the CLI is the brush the slider says 40. */
	const SPAN = 1000;

	let canvas = $state<HTMLCanvasElement | null>(null);
	let hex = $state('#4aa3ff');
	let size = $state(24);
	let soft = $state(0);
	let erasing = $state(false);
	let drawing = false;
	let last: { x: number; y: number } | null = null;
	let mine: string | null = null;

	$effect(() => {
		const url = value;
		const el = canvas;
		const echo = url === mine;
		mine = null;
		if (!el || echo) return;
		const ctx = el.getContext('2d');
		if (!ctx) return;
		if (!url) {
			ctx.clearRect(0, 0, SIZE, SIZE);
			return;
		}
		const img = new Image();
		img.onload = () => {
			ctx.clearRect(0, 0, SIZE, SIZE);
			ctx.drawImage(img, 0, 0, SIZE, SIZE);
		};
		img.src = url;
		return () => { img.onload = null; };
	});

	function at(e: PointerEvent): { x: number; y: number } | null {
		if (!canvas) return null;
		const box = canvas.getBoundingClientRect();
		return {
			x: ((e.clientX - box.left) / box.width) * SPAN,
			y: ((e.clientY - box.top) / box.height) * SPAN
		};
	}

	/** ONE stroke, whatever asked for it. A hand at the pad and a turtle step from the CLI both
	    arrive here, so there is no second painter to disagree with this one. */
	function paint(
		from: { x: number; y: number },
		to: { x: number; y: number },
		ink: string,
		width: number,
		softness: number
	): void {
		const ctx = canvas?.getContext('2d');
		if (!ctx) return;
		const k = SIZE / SPAN;
		ctx.save();
		ctx.globalCompositeOperation = ink === 'erase' ? 'destination-out' : 'source-over';
		ctx.strokeStyle = ink === 'erase' ? '#000' : ink;
		ctx.lineWidth = Math.max(width * k, 0.5);
		ctx.lineCap = 'round';
		ctx.lineJoin = 'round';
		// `filter` is what makes a soft brush soft. Where it is unsupported the stroke is simply
		// hard-edged, which is a lesser brush and never a broken one.
		if (softness > 0) ctx.filter = `blur(${softness * k}px)`;
		ctx.beginPath();
		ctx.moveTo(from.x * k, from.y * k);
		ctx.lineTo(to.x * k, to.y * k);
		ctx.stroke();
		ctx.restore();
	}

	function stroke(from: { x: number; y: number }, to: { x: number; y: number }): void {
		paint(from, to, erasing ? 'erase' : hex, size, soft);
	}

	/** A turtle script's strokes, made and then committed as ONE change — the script is a
	    submission, so the pad answers it the way a pointer answers a gesture, not a move. */
	let done = $state(0);
	$effect(() => {
		const batch = pending;
		if (!batch || batch.id === done || !canvas) return;
		done = batch.id;
		for (const m of batch.marks) {
			if (m.mark === 'clear') wipe();
			else paint({ x: m.from[0], y: m.from[1] }, { x: m.to[0], y: m.to[1] }, m.ink, m.width, m.soft);
		}
		commit();
	});

	function commit(): void {
		if (!canvas) return;
		mine = canvas.toDataURL('image/png');
		onChange(mine);
	}

	function down(e: PointerEvent): void {
		if (disabled) return;
		const p = at(e);
		if (!p) return;
		drawing = true;
		last = p;
		(e.currentTarget as HTMLCanvasElement).setPointerCapture(e.pointerId);
		// A tap is a dot, so the shortest stroke still leaves a mark.
		stroke(p, { x: p.x + 0.01, y: p.y });
		e.preventDefault();
	}
	function move(e: PointerEvent): void {
		if (!drawing || !last) return;
		const p = at(e);
		if (!p) return;
		stroke(last, p);
		last = p;
	}
	function up(): void {
		if (!drawing) return;
		drawing = false;
		last = null;
		commit();
	}
	function wipe(): void {
		canvas?.getContext('2d')?.clearRect(0, 0, SIZE, SIZE);
	}
	function clear(): void {
		if (disabled) return;
		wipe();
		commit();
	}
</script>

<div class="pad" data-testid="draw-pad">
	<div class="tools">
		<label class="swatch" title="Pick the ink" style={`--ink: ${hex}`}>
			<input
				type="color"
				value={hex}
				{disabled}
				data-testid="draw-colour"
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
				data-testid="draw-size"
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
				data-testid="draw-soft"
			/>
		</label>
		<Button
			size="sm"
			variant={erasing ? 'primary' : 'ghost'}
			title="Paint transparency instead of colour"
			{disabled}
			data-testid="draw-eraser"
			onclick={() => (erasing = !erasing)}>erase</Button
		>
		<Button
			size="sm"
			variant="ghost"
			title="Clear the drawing"
			{disabled}
			data-testid="draw-clear"
			onclick={clear}>clear</Button
		>
	</div>
	<canvas
		bind:this={canvas}
		class="sheet"
		width={SIZE}
		height={SIZE}
		data-testid="draw-canvas"
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
