<!-- /dev/graphics — the graphics engine in the browser: one shipped generator, drawn by the
     engine crate compiled to wasm, on a WebGPU canvas. `--debug` opens it, as it opens /dev/ui. -->
<script lang="ts">
	import { onMount } from 'svelte';

	/** A shipped generator — no inputs, no state — so the page needs nothing but its text;
	 *  `?type=graphics:Noise` picks another. */
	const TYPE = new URLSearchParams(location.search).get('type') ?? 'graphics:Ramp';
	/** What `backend/graphics/goofi-graphics-web/build.sh` writes. */
	const GLUE = '/dev/graphics/goofi_graphics_web.js';

	let canvas: HTMLCanvasElement;
	let message = $state('starting');
	let frames = $state(0);

	/** The node file's text, through the same op the editor reads it with. */
	async function source(type: string): Promise<string> {
		const response = await fetch('/exec', {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify({ commands: [`library get ${type} --source`] })
		});
		if (!response.ok) throw new Error(`/exec answered ${response.status}`);
		const body = await response.json();
		const text = body?.results?.[0]?.result?.text;
		if (typeof text !== 'string') throw new Error(`${type} has no source text`);
		return text;
	}

	onMount(() => {
		let stop = false;
		let raf = 0;
		(async () => {
			if (!('gpu' in navigator)) {
				message = 'no navigator.gpu: the browser engine needs WebGPU';
				return;
			}
			let glue;
			try {
				glue = await import(/* @vite-ignore */ GLUE);
				await glue.default();
			} catch (e) {
				message = `no engine build at ${GLUE}; run backend/graphics/goofi-graphics-web/build.sh (${e})`;
				return;
			}
			const text = await source(TYPE);
			const stage = await glue.start(canvas, TYPE, text);
			message = `${TYPE} on WebGPU`;
			const loop = (now: number) => {
				if (stop) return;
				const scale = window.devicePixelRatio || 1;
				const w = Math.max(1, Math.round(canvas.clientWidth * scale));
				const h = Math.max(1, Math.round(canvas.clientHeight * scale));
				if (canvas.width !== w) canvas.width = w;
				if (canvas.height !== h) canvas.height = h;
				stage.tick(now / 1000);
				frames += 1;
				raf = requestAnimationFrame(loop);
			};
			raf = requestAnimationFrame(loop);
		})().catch((e) => {
			message = String(e?.message ?? e);
		});
		return () => {
			stop = true;
			cancelAnimationFrame(raf);
		};
	});
</script>

<main>
	<p data-testid="graphics-message">{message}</p>
	<canvas bind:this={canvas} data-testid="graphics-canvas" data-frames={frames}></canvas>
</main>

<style>
	main {
		display: grid;
		gap: 0.5rem;
		padding: 1rem;
	}
	canvas {
		width: min(100%, 640px);
		aspect-ratio: 16 / 9;
		background: #000;
	}
</style>
