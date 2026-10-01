/** Thin-plate spline weights for EEG topomaps, as MNE's `plot_topomap`; the plot surface evaluates
 * the field. A ring outside the head carries its neighbours' mean, as MNE's `border="mean"`. */
import { HEAD_RADIUS } from './eegLayout';

const TPS_EPS_SQ = 1e-12;
/** The extra points on the ring outside the head. */
const RING = 16;

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

/** Writes the ring at `radius` into the tail of `posX`/`posY`; each ring point's `k` nearest real channels. */
function buildRing(posX: Float64Array, posY: Float64Array, nReal: number, radius: number, k: number): number[][] {
	const count = posX.length - nReal;
	const nn: number[][] = new Array(count);
	const buf: { i: number; d: number }[] = new Array(nReal);
	for (let i = 0; i < count; i++) {
		const theta = (i * 2 * Math.PI) / count - Math.PI / 2;
		const x = (posX[nReal + i] = 0.5 + Math.cos(theta) * radius);
		const y = (posY[nReal + i] = 0.5 + Math.sin(theta) * radius);
		for (let j = 0; j < nReal; j++) buf[j] = { i: j, d: (posX[j] - x) ** 2 + (posY[j] - y) ** 2 };
		buf.sort((a, b) => a.d - b.d);
		nn[i] = buf.slice(0, k).map((b) => b.i);
	}
	return nn;
}

export function buildLayout(channelPositions: ReadonlyArray<readonly [number, number]>, layoutKey: string): TopoLayout {
	const nReal = channelPositions.length;
	const nTotal = nReal + RING;
	const posX = new Float64Array(nTotal);
	const posY = new Float64Array(nTotal);
	channelPositions.forEach(([x, y], i) => {
		posX[i] = x;
		posY[i] = y;
	});
	const extraNN = buildRing(posX, posY, nReal, 0.52, 3);

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
	return { nReal, nExtra: RING, posX, posY, extraNN, Minv, layoutKey };
}

/** The electrodes' place inside a `w`×`h` box: the centre, the side of the centred square the
 * [0, 1]² layout fills, and the head's radius in px. */
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
