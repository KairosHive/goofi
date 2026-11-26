import { FieldPlot } from './field';
import { program, type Program } from './gl';
import { ImagePlot } from './image';
import { LinePlot } from './line';
import { clipped, damage, deviceRect, meets, type Rect, type View } from './math';
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
	/** The whole pane is due: its view moved, or the context came back. */
	all: boolean;
	/** The plots that changed since the last frame. */
	touched: Set<Plot>;
	/** Device rects whose picture is out of date: where a touched or removed plot was drawn. */
	stale: Rect[];
	/** Where each plot was last drawn, inside the pane. */
	painted: Map<Plot, Rect>;
}

/** One rect of one pane that a frame draws again. */
interface Job {
	pane: Pane;
	r: Rect;
}

/** The atlas grows in these steps, so a pane that grows by a pixel does not reallocate it. */
const STEP = 256;
/** The atlas shrinks after this many frames in a row that a quarter of it would have held. */
const LOOSE = 120;

/** One WebGL2 context for every surface it makes, on the main thread or in a worker: a browser
 * allows a thread few contexts, and a worker fewer. A frame draws the rects that changed side by
 * side on one atlas, and one bitmap handover brings each to its canvas. Throws without WebGL2. */
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
	let loose = 0;
	let lost = false;
	let disposed = false;

	// A worker without an animation frame of its own draws on a timer at a display's pace.
	const schedule =
		typeof requestAnimationFrame === 'function'
			? requestAnimationFrame
			: (fn: () => void): number => setTimeout(fn, 16) as unknown as number;
	const invalidate = (): void => {
		if (scheduled || lost || disposed) return;
		scheduled = true;
		schedule(draw);
	};

	/** The atlas at least the sheet's size, in steps; shrunk once it stays four times too large. */
	function fit(width: number, height: number): void {
		const w = Math.min(limit, Math.ceil(width / STEP) * STEP);
		const h = Math.min(limit, Math.ceil(height / STEP) * STEP);
		loose = atlas.width * atlas.height > 4 * w * h ? loose + 1 : 0;
		const shrink = loose >= LOOSE;
		if (shrink) loose = 0;
		if (atlas.width < w || shrink) atlas.width = w;
		if (atlas.height < h || shrink) atlas.height = h;
	}

	/** What `pane` owes this frame, and its state reset: the whole pane, or the rects that changed. */
	function due(pane: Pane): Rect[] {
		const { w, h, view } = pane;
		let rects: Rect[];
		if (pane.all) {
			pane.painted.clear();
			rects = [{ x: 0, y: 0, w, h }];
		} else {
			for (const p of pane.touched) {
				pane.painted.delete(p);
				const now = clipped(deviceRect(p.rect, view), w, h);
				if (now) pane.stale.push(now);
			}
			rects = damage(pane.stale);
		}
		pane.all = false;
		pane.touched.clear();
		pane.stale = [];
		return rects;
	}

	function draw(): void {
		scheduled = false;
		if (lost || disposed) return;
		const jobs: Job[] = [];
		for (const pane of panes) for (const r of due(pane)) jobs.push({ pane, r });
		for (const sheet of pack(jobs.map(({ r }) => [r.w, r.h]), limit)) {
			fit(sheet.width, sheet.height);
			const W = atlas.width;
			const H = atlas.height;
			gl.viewport(0, 0, W, H);
			gl.enable(gl.BLEND);
			gl.blendFuncSeparate(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA, gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
			gl.enable(gl.SCISSOR_TEST);
			for (const [i, slot] of sheet.slots) paint(jobs[i], slot.x, slot.y, W, H);
			const bitmap = atlas.transferToImageBitmap();
			for (const [i, slot] of sheet.slots) {
				const { pane, r } = jobs[i];
				pane.target.clearRect(r.x, r.y, r.w, r.h);
				pane.target.drawImage(bitmap, slot.x, slot.y, r.w, r.h, r.x, r.y, r.w, r.h);
			}
			bitmap.close();
		}
	}

	/** Draw rect `r` of its pane with its top-left at atlas pixel (sx, sy), y down; the atlas is W × H. */
	function paint({ pane, r }: Job, sx: number, sy: number, W: number, H: number): void {
		const { w, h, view } = pane;
		gl.scissor(sx, H - sy - r.h, r.w, r.h);
		gl.clearColor(0, 0, 0, 0);
		gl.clear(gl.COLOR_BUFFER_BIT);
		const size: [number, number] = [W, H];
		// Where the pane's own top-left falls on the atlas, so the geometry keeps the whole rect.
		const ox = sx - r.x;
		const oy = sy - r.y;
		for (const p of [...pane.plots].sort((a, b) => a.order - b.order)) {
			const d = deviceRect(p.rect, view);
			const inside = clipped(d, w, h);
			if (!inside || !meets(inside, r)) continue;
			pane.painted.set(p, inside);
			// A plot under the rect draws the part of itself the rect holds.
			const c = clipped({ x: inside.x - r.x, y: inside.y - r.y, w: inside.w, h: inside.h }, r.w, r.h)!;
			gl.scissor(sx + c.x, H - sy - c.y - c.h, c.w, c.h);
			// A transparent background leaves what an earlier plot drew under the rect.
			if (p.background[3] > 0) {
				gl.clearColor(...p.background);
				gl.clear(gl.COLOR_BUFFER_BIT);
			}
			const t = { x: ox + d.x, y: oy + d.y, w: d.w, h: d.h };
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
		for (const pane of panes) pane.all = true;
		invalidate();
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
			all: true,
			touched: new Set(),
			stale: [],
			painted: new Map()
		};
		panes.add(pane);
		/** `p` changed: where it was drawn is out of date, and so is where it is now. */
		const touch = (p: Plot): void => {
			const was = pane.painted.get(p);
			if (was) pane.stale.push(was);
			pane.touched.add(p);
			invalidate();
		};
		const detach = (p: Plot): void => {
			touch(p);
			pane.plots.delete(p);
			pane.touched.delete(p);
			pane.painted.delete(p);
		};
		const add = <P extends Plot>(make: (touch: () => void) => P): P => {
			const p: P = make(() => touch(p));
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
				pane.all = true;
				invalidate();
			},
			addLine: () => add((t) => new LinePlot(t, detach)),
			addImage: () => add((t) => new ImagePlot(t, detach)),
			addPath: () => add((t) => new PathPlot(t, detach)),
			addField: () => add((t) => new FieldPlot(t, detach)),
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
