import { FieldPlot } from './field';
import { program, type Program } from './gl';
import { ImagePlot } from './image';
import { LinePlot } from './line';
import { deviceRect, visible, type View } from './math';
import { pack } from './pack';
import { PathPlot } from './path';
import { FIELD_FS, IMAGE_FS, IMAGE_VS, LINE_FS, LINE_VS, PATH_FS, PATH_VS } from './shaders';

export type Plot = LinePlot | ImagePlot | PathPlot | FieldPlot;

export interface Surface {
	/** The pane and the camera; the canvas backing store follows `width × dpr`. */
	setView(v: View): void;
	addLine(): LinePlot;
	addImage(): ImagePlot;
	addPath(): PathPlot;
	addField(): FieldPlot;
	dispose(): void;
}

export interface Renderer {
	/** A surface on `canvas`, drawn by this renderer. The canvas takes a 2D context, so it must
	 * not hold a context of another kind. */
	surface(canvas: HTMLCanvasElement | OffscreenCanvas): Surface;
	dispose(): void;
}

type Target = CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;

interface Pane {
	canvas: HTMLCanvasElement | OffscreenCanvas;
	target: Target;
	plots: Set<Plot>;
	view: View;
	/** The pane in device pixels, each side within the atlas limit. */
	w: number;
	h: number;
	dirty: boolean;
}

/** The atlas grows in these steps, so a pane that grows by a pixel does not reallocate it. */
const STEP = 256;

/** One WebGL2 context for every surface it makes, on the main thread or in a worker: a browser
 * allows a thread few contexts, and a worker fewer. Every frame draws the surfaces that changed
 * side by side on one atlas, and one bitmap handover brings each its own picture. Throws when
 * WebGL2 is not available. */
export function createRenderer(): Renderer {
	const atlas = new OffscreenCanvas(STEP, STEP);
	const ctx = atlas.getContext('webgl2', { antialias: true, depth: false, stencil: false });
	if (!ctx) throw new Error('plotluck: WebGL2 is not available');
	const gl: WebGL2RenderingContext = ctx;
	const limit = Math.min(
		gl.getParameter(gl.MAX_TEXTURE_SIZE) as number,
		gl.getParameter(gl.MAX_RENDERBUFFER_SIZE) as number
	);
	let programs = build(gl);
	const panes = new Set<Pane>();
	let scheduled = false;
	let lost = false;
	let disposed = false;

	// A worker without an animation frame of its own draws on a timer at a display's pace.
	const schedule =
		typeof requestAnimationFrame === 'function'
			? requestAnimationFrame
			: (fn: () => void): number => setTimeout(fn, 16) as unknown as number;
	const invalidate = (pane: Pane): void => {
		pane.dirty = true;
		if (scheduled || lost || disposed) return;
		scheduled = true;
		schedule(draw);
	};

	/** The atlas at least the sheet's size, in steps; shrunk once it is four times too large. */
	function fit(width: number, height: number): void {
		const w = Math.min(limit, Math.ceil(width / STEP) * STEP);
		const h = Math.min(limit, Math.ceil(height / STEP) * STEP);
		const loose = atlas.width * atlas.height > 4 * w * h;
		if (atlas.width < w || loose) atlas.width = w;
		if (atlas.height < h || loose) atlas.height = h;
	}

	function draw(): void {
		scheduled = false;
		if (lost || disposed) return;
		// Every pane keeps its slot while the set stands, so a frame of one pane reallocates nothing.
		const live = [...panes];
		for (const sheet of pack(live.map((p) => [p.w, p.h]), limit)) {
			const due = [...sheet.slots].filter(([i]) => live[i].dirty);
			if (due.length === 0) continue;
			fit(sheet.width, sheet.height);
			const W = atlas.width;
			const H = atlas.height;
			gl.viewport(0, 0, W, H);
			gl.enable(gl.BLEND);
			gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
			gl.enable(gl.SCISSOR_TEST);
			for (const [i, slot] of due) paint(live[i], slot.x, slot.y, W, H);
			const bitmap = atlas.transferToImageBitmap();
			for (const [i, slot] of due) {
				const pane = live[i];
				pane.dirty = false;
				// A resize resets the 2D state, so the mode is set with every handover.
				pane.target.globalCompositeOperation = 'copy';
				pane.target.drawImage(bitmap, slot.x, slot.y, pane.w, pane.h, 0, 0, pane.w, pane.h);
			}
			bitmap.close();
		}
	}

	/** Draw `pane` with its top-left at atlas pixel (sx, sy), y down; the atlas is W × H. */
	function paint(pane: Pane, sx: number, sy: number, W: number, H: number): void {
		const { w, h, view } = pane;
		gl.scissor(sx, H - sy - h, w, h);
		gl.clearColor(0, 0, 0, 0);
		gl.clear(gl.COLOR_BUFFER_BIT);
		const size: [number, number] = [W, H];
		for (const p of [...pane.plots].sort((a, b) => a.order - b.order)) {
			const d = deviceRect(p.rect, view);
			if (!visible(d, view)) continue;
			// The scissor is the rect inside the pane; the geometry keeps the whole rect.
			const cx = Math.max(d.x, 0);
			const cy = Math.max(d.y, 0);
			const cw = Math.min(d.x + d.w, w) - cx;
			const ch = Math.min(d.y + d.h, h) - cy;
			if (cw <= 0 || ch <= 0) continue;
			gl.scissor(sx + cx, H - sy - cy - ch, cw, ch);
			// A transparent background leaves what an earlier plot drew under the rect.
			if (p.background[3] > 0) {
				gl.clearColor(...p.background);
				gl.clear(gl.COLOR_BUFFER_BIT);
			}
			const t = { x: sx + d.x, y: sy + d.y, w: d.w, h: d.h };
			if (p instanceof LinePlot) {
				gl.bindVertexArray(programs.line.vao);
				p.draw(gl, programs.line, t, size, view.dpr);
			} else if (p instanceof PathPlot) {
				gl.bindVertexArray(programs.path.vao);
				p.draw(gl, programs.path, t, size, view.dpr);
			} else {
				gl.bindVertexArray(null);
				p.draw(gl, p instanceof ImagePlot ? programs.image : programs.field, t, size);
			}
		}
	}

	// The CPU copies outlive the context: on restore the programs and every plot's handles are rebuilt.
	atlas.addEventListener('webglcontextlost', (e) => {
		e.preventDefault();
		lost = true;
		for (const pane of panes) for (const p of pane.plots) p.release();
	});
	atlas.addEventListener('webglcontextrestored', () => {
		try {
			programs = build(gl);
		} catch (err) {
			console.warn(err);
			return;
		}
		lost = false;
		for (const pane of panes) invalidate(pane);
	});

	function surface(canvas: HTMLCanvasElement | OffscreenCanvas): Surface {
		const target = canvas.getContext('2d') as Target | null;
		if (!target) throw new Error('plotluck: the canvas gives no 2D context');
		const pane: Pane = {
			canvas,
			target,
			plots: new Set(),
			view: { x: 0, y: 0, zoom: 1, width: 0, height: 0, dpr: 1 },
			w: 0,
			h: 0,
			dirty: false
		};
		panes.add(pane);
		const touch = (): void => invalidate(pane);
		const detach = (p: Plot): void => {
			pane.plots.delete(p);
			touch();
		};
		const add = <P extends Plot>(p: P): P => {
			pane.plots.add(p);
			return p;
		};
		return {
			setView(v) {
				pane.view = v;
				pane.w = Math.min(limit, Math.round(v.width * v.dpr));
				pane.h = Math.min(limit, Math.round(v.height * v.dpr));
				if (canvas.width !== pane.w) canvas.width = pane.w;
				if (canvas.height !== pane.h) canvas.height = pane.h;
				touch();
			},
			addLine: () => add(new LinePlot(touch, detach)),
			addImage: () => add(new ImagePlot(touch, detach)),
			addPath: () => add(new PathPlot(touch, detach)),
			addField: () => add(new FieldPlot(touch, detach)),
			dispose() {
				for (const p of pane.plots) p.release();
				pane.plots.clear();
				panes.delete(pane);
				target.clearRect(0, 0, canvas.width, canvas.height);
			}
		};
	}

	return {
		surface,
		dispose() {
			disposed = true;
			for (const pane of panes) {
				for (const p of pane.plots) p.release();
				pane.plots.clear();
			}
			panes.clear();
			gl.getExtension('WEBGL_lose_context')?.loseContext();
		}
	};
}

/** A program with instanced attributes keeps them in a VAO of its own, so a quad drawn after a
 * removed line never meets an attribute enabled on a deleted buffer. */
function build(gl: WebGL2RenderingContext): { line: Program; image: Program; path: Program; field: Program } {
	const line = { ...program(gl, LINE_VS, LINE_FS), vao: gl.createVertexArray() };
	const path = { ...program(gl, PATH_VS, PATH_FS), vao: gl.createVertexArray() };
	return { line, path, image: program(gl, IMAGE_VS, IMAGE_FS), field: program(gl, IMAGE_VS, FIELD_FS) };
}
