import { program, type Program } from './gl';
import { ImagePlot } from './image';
import { LinePlot } from './line';
import { deviceRect, visible, type View } from './math';
import { IMAGE_FS, IMAGE_VS, LINE_FS, LINE_VS } from './shaders';

export type Plot = LinePlot | ImagePlot;

export interface Surface {
	/** The pane and the camera; the canvas backing store follows `width × dpr`. */
	setView(v: View): void;
	addLine(): LinePlot;
	addImage(): ImagePlot;
	dispose(): void;
}

/** One surface per host canvas. Throws when the canvas cannot give a WebGL2 context. */
export function createSurface(canvas: HTMLCanvasElement): Surface {
	const ctx = canvas.getContext('webgl2', { antialias: true, depth: false, stencil: false });
	if (!ctx) throw new Error('glance: WebGL2 is not available');
	const gl: WebGL2RenderingContext = ctx;
	let programs = build(gl);
	const plots = new Set<Plot>();
	let view: View = { x: 0, y: 0, zoom: 1, width: 0, height: 0, dpr: 1 };
	let dirty = false;
	let lost = false;
	let disposed = false;

	const invalidate = (): void => {
		if (dirty || lost || disposed) return;
		dirty = true;
		requestAnimationFrame(draw);
	};
	const detach = (p: Plot): void => {
		plots.delete(p);
		invalidate();
	};
	const add = <P extends Plot>(p: P): P => {
		plots.add(p);
		return p;
	};

	function draw(): void {
		dirty = false;
		if (lost || disposed) return;
		const w = canvas.width;
		const h = canvas.height;
		if (w === 0 || h === 0) return;
		gl.disable(gl.SCISSOR_TEST);
		gl.viewport(0, 0, w, h);
		gl.clearColor(0, 0, 0, 0);
		gl.clear(gl.COLOR_BUFFER_BIT);
		gl.enable(gl.BLEND);
		gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
		gl.enable(gl.SCISSOR_TEST);
		const size: [number, number] = [w, h];
		for (const p of [...plots].sort((a, b) => a.order - b.order)) {
			const d = deviceRect(p.rect, view);
			if (!visible(d, view)) continue;
			gl.scissor(d.x, h - d.y - d.h, d.w, d.h);
			gl.clearColor(...p.background);
			gl.clear(gl.COLOR_BUFFER_BIT);
			if (p instanceof LinePlot) p.draw(gl, programs.line, d, size, view.dpr);
			else p.draw(gl, programs.image, d, size);
		}
	}

	// The CPU copies outlive the context: on restore the programs and every plot's handles are rebuilt.
	canvas.addEventListener('webglcontextlost', (e) => {
		e.preventDefault();
		lost = true;
	});
	canvas.addEventListener('webglcontextrestored', () => {
		try {
			programs = build(gl);
		} catch (err) {
			console.warn(err);
			return;
		}
		lost = false;
		invalidate();
	});

	return {
		setView(v) {
			view = v;
			const w = Math.round(v.width * v.dpr);
			const h = Math.round(v.height * v.dpr);
			if (canvas.width !== w) canvas.width = w;
			if (canvas.height !== h) canvas.height = h;
			invalidate();
		},
		addLine: () => add(new LinePlot(invalidate, detach)),
		addImage: () => add(new ImagePlot(invalidate, detach)),
		dispose() {
			disposed = true;
			for (const p of plots) p.release();
			plots.clear();
			gl.getExtension('WEBGL_lose_context')?.loseContext();
		}
	};
}

function build(gl: WebGL2RenderingContext): { line: Program; image: Program } {
	return { line: program(gl, LINE_VS, LINE_FS), image: program(gl, IMAGE_VS, IMAGE_FS) };
}
