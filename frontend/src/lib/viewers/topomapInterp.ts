/**
 * Thin-plate spline interpolation for EEG topomaps, approximating MNE's `plot_topomap`.
 * The ring of extra points outside the head carries its neighbours' mean, as MNE's `border="mean"`.
 * The weights are solved here; the field is evaluated per pixel by the plot surface.
 */

const TPS_EPS_SQ = 1e-12;

function tpsKernel(r2: number): number {
	if (r2 <= TPS_EPS_SQ) return 0;
	return 0.5 * r2 * Math.log(r2);
}

export interface TopoLayout {
	nReal: number;
	nExtra: number;
	posX: Float64Array;
	posY: Float64Array;
	/** For each extra ring point, indices into the real-channel array whose mean becomes its value. */
	extraNN: number[][];
	/** Inverse of the augmented matrix; (nTotal+3)² row-major. */
	Minv: Float64Array;
	layoutKey: string;
}

function invertMatrix(m: Float64Array, n: number): Float64Array {
	// Gauss-Jordan elimination with partial pivoting on the augmented [M | I] matrix.
	const stride = 2 * n;
	const a = new Float64Array(n * stride);
	for (let i = 0; i < n; i++) {
		for (let j = 0; j < n; j++) a[i * stride + j] = m[i * n + j];
		a[i * stride + n + i] = 1;
	}
	for (let i = 0; i < n; i++) {
		let pivotRow = i;
		let pivotVal = Math.abs(a[i * stride + i]);
		for (let k = i + 1; k < n; k++) {
			const v = Math.abs(a[k * stride + i]);
			if (v > pivotVal) {
				pivotVal = v;
				pivotRow = k;
			}
		}
		if (pivotVal < 1e-12) throw new Error('topomap TPS: singular matrix');
		if (pivotRow !== i) {
			for (let j = 0; j < stride; j++) {
				const t = a[i * stride + j];
				a[i * stride + j] = a[pivotRow * stride + j];
				a[pivotRow * stride + j] = t;
			}
		}
		const pv = a[i * stride + i];
		const invPv = 1 / pv;
		for (let j = 0; j < stride; j++) a[i * stride + j] *= invPv;
		for (let k = 0; k < n; k++) {
			if (k === i) continue;
			const f = a[k * stride + i];
			if (f === 0) continue;
			for (let j = 0; j < stride; j++) a[k * stride + j] -= f * a[i * stride + j];
		}
	}
	const out = new Float64Array(n * n);
	for (let i = 0; i < n; i++) {
		for (let j = 0; j < n; j++) out[i * n + j] = a[i * stride + n + j];
	}
	return out;
}

function buildRing(
	realX: Float64Array,
	realY: Float64Array,
	count: number,
	radius: number,
	k: number
): { x: Float64Array; y: Float64Array; nn: number[][] } {
	const x = new Float64Array(count);
	const y = new Float64Array(count);
	const nn: number[][] = new Array(count);
	const nReal = realX.length;
	const kk = Math.min(k, nReal);
	const buf: { i: number; d: number }[] = new Array(nReal);
	for (let i = 0; i < count; i++) {
		const theta = (i * 2 * Math.PI) / count - Math.PI / 2;
		x[i] = 0.5 + Math.cos(theta) * radius;
		y[i] = 0.5 + Math.sin(theta) * radius;
		for (let j = 0; j < nReal; j++) {
			const dx = realX[j] - x[i];
			const dy = realY[j] - y[i];
			buf[j] = { i: j, d: dx * dx + dy * dy };
		}
		buf.sort((a, b) => a.d - b.d);
		const lst: number[] = new Array(kk);
		for (let j = 0; j < kk; j++) lst[j] = buf[j].i;
		nn[i] = lst;
	}
	return { x, y, nn };
}

export function buildLayout(
	channelPositions: ReadonlyArray<readonly [number, number]>,
	layoutKey: string,
	opts: { extraCount?: number; extraRadius?: number; knnForBorder?: number } = {}
): TopoLayout | null {
	const nReal = channelPositions.length;
	if (nReal < 3) return null;
	const extraCount = opts.extraCount ?? 16;
	const extraRadius = opts.extraRadius ?? 0.52;
	const knn = opts.knnForBorder ?? 3;

	const realX = new Float64Array(nReal);
	const realY = new Float64Array(nReal);
	for (let i = 0; i < nReal; i++) {
		realX[i] = channelPositions[i][0];
		realY[i] = channelPositions[i][1];
	}
	const ring = buildRing(realX, realY, extraCount, extraRadius, knn);
	const nExtra = ring.nn.length;
	const nTotal = nReal + nExtra;

	const posX = new Float64Array(nTotal);
	const posY = new Float64Array(nTotal);
	for (let i = 0; i < nReal; i++) {
		posX[i] = realX[i];
		posY[i] = realY[i];
	}
	for (let i = 0; i < nExtra; i++) {
		posX[nReal + i] = ring.x[i];
		posY[nReal + i] = ring.y[i];
	}

	const dim = nTotal + 3;
	const M = new Float64Array(dim * dim);
	for (let i = 0; i < nTotal; i++) {
		for (let j = 0; j < nTotal; j++) {
			if (i === j) continue;
			const dx = posX[i] - posX[j];
			const dy = posY[i] - posY[j];
			M[i * dim + j] = tpsKernel(dx * dx + dy * dy);
		}
		M[i * dim + nTotal + 0] = 1;
		M[i * dim + nTotal + 1] = posX[i];
		M[i * dim + nTotal + 2] = posY[i];
		M[(nTotal + 0) * dim + i] = 1;
		M[(nTotal + 1) * dim + i] = posX[i];
		M[(nTotal + 2) * dim + i] = posY[i];
	}

	const Minv = invertMatrix(M, dim);
	return { nReal, nExtra, posX, posY, extraNN: ring.nn, Minv, layoutKey };
}

/** The head's radius in the layout's unit square, and the electrodes' place inside a `w`×`h` box:
 * the centre, and the side of the centred square the [0, 1]² layout fills. */
export const HEAD_RADIUS = 0.45;
export function headFrame(w: number, h: number): { cx: number; cy: number; side: number; radius: number } {
	const side = Math.min(w, h);
	return { cx: w / 2, cy: h / 2, side, radius: side * HEAD_RADIUS };
}

export function solveWeights(layout: TopoLayout, realVals: ArrayLike<number>): Float64Array {
	const { nReal, nExtra, extraNN, Minv } = layout;
	const nTotal = nReal + nExtra;
	const dim = nTotal + 3;
	const rhs = new Float64Array(dim);
	for (let i = 0; i < nReal; i++) rhs[i] = realVals[i];
	for (let i = 0; i < nExtra; i++) {
		const nn = extraNN[i];
		let s = 0;
		for (let j = 0; j < nn.length; j++) s += realVals[nn[j]];
		rhs[nReal + i] = s / nn.length;
	}
	const w = new Float64Array(dim);
	for (let i = 0; i < dim; i++) {
		let s = 0;
		for (let j = 0; j < dim; j++) s += Minv[i * dim + j] * rhs[j];
		w[i] = s;
	}
	return w;
}

/** The solved spline as the surface takes it: every centre with its weight, and the affine part. */
export function fieldCentres(
	layout: TopoLayout,
	weights: Float64Array
): { points: Float32Array; affine: [number, number, number] } {
	const nTotal = layout.nReal + layout.nExtra;
	const points = new Float32Array(nTotal * 3);
	for (let i = 0; i < nTotal; i++) {
		points[i * 3] = layout.posX[i];
		points[i * 3 + 1] = layout.posY[i];
		points[i * 3 + 2] = weights[i];
	}
	return { points, affine: [weights[nTotal], weights[nTotal + 1], weights[nTotal + 2]] };
}

/** The field at one layout-space point `(x, y)`, for a readout off the grid. */
export function evaluateAt(layout: TopoLayout, weights: Float64Array, x: number, y: number): number {
	const nTotal = layout.nReal + layout.nExtra;
	let v = weights[nTotal] + weights[nTotal + 1] * x + weights[nTotal + 2] * y;
	for (let i = 0; i < nTotal; i++) {
		const dx = x - layout.posX[i];
		const dy = y - layout.posY[i];
		v += weights[i] * tpsKernel(dx * dx + dy * dy);
	}
	return v;
}
