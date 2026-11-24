//! Specparam — the aperiodic 1/f fit of a spectrum and the peaks that rise over it, per channel.
//! A port of specparam's spectral fit algorithm (https://specparam-tools.github.io/), on the
//! `Psd` frame it takes: `[.., F]` with the frequencies on its last axis.

use goofi_core::{stream, Axes, Coord, Data, SlotType};
use goofi_signal_sdk::{Face, Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, Params, ParamSpec, SlotDecl, Tag};

/// specparam's private settings, at their defaults.
const AP_PERCENTILE_THRESH: f64 = 0.025;
const CF_BOUND: f64 = 1.5;
const BW_STD_EDGE: f64 = 1.0;
const GAUSS_OVERLAP_THRESH: f64 = 0.75;
const MAXFEV: usize = 5000;
const TOL: f64 = 1e-5;

/// What one fit takes: specparam's public settings by their names.
#[derive(Clone, Copy)]
pub struct Settings {
    pub knee: bool,
    pub peak_width_limits: (f64, f64),
    pub max_n_peaks: usize,
    pub min_peak_height: f64,
    pub peak_threshold: f64,
}

/// One fitted spectrum: the aperiodic params (offset, [knee,] exponent), the gaussians as
/// (cf, pw, bw), and both components in log10 power over the fitted frequencies.
pub struct Fit {
    pub aperiodic: Vec<f64>,
    pub gaussians: Vec<[f64; 3]>,
    pub ap_fit: Vec<f64>,
    pub peak_fit: Vec<f64>,
}

/// The bins of `freqs` inside `range`, as specparam trims: inclusive, and never 0 Hz.
pub fn bins_within(freqs: &[f64], range: (f64, f64)) -> Vec<usize> {
    (0..freqs.len()).filter(|&i| freqs[i] >= range.0 && freqs[i] <= range.1 && freqs[i] != 0.0).collect()
}

fn aperiodic(freqs: &[f64], p: &[f64]) -> Vec<f64> {
    match p {
        [offset, knee, exp] => freqs.iter().map(|f| offset - (knee + f.powf(*exp)).log10()).collect(),
        [offset, exp] => freqs.iter().map(|f| offset - f.powf(*exp).log10()).collect(),
        _ => unreachable!("two or three aperiodic params"),
    }
}

fn gaussians(freqs: &[f64], p: &[f64]) -> Vec<f64> {
    let mut ys = vec![0.0; freqs.len()];
    for g in p.chunks_exact(3) {
        let (ctr, hgt, wid) = (g[0], g[1], g[2]);
        for (y, f) in ys.iter_mut().zip(freqs) {
            *y += hgt * (-(f - ctr).powi(2) / (2.0 * wid * wid)).exp();
        }
    }
    ys
}

/// Residuals `model(x) - y` and the Jacobian, row-major `[m, n]`, of the aperiodic function.
fn aperiodic_residual(freqs: &[f64], y: &[f64], p: &[f64], r: &mut [f64], j: &mut [f64]) {
    let ln10 = std::f64::consts::LN_10;
    for (i, f) in freqs.iter().enumerate() {
        let row = &mut j[i * p.len()..(i + 1) * p.len()];
        row[0] = 1.0;
        match p {
            [offset, knee, exp] => {
                let fe = f.powf(*exp);
                r[i] = offset - (knee + fe).log10() - y[i];
                row[1] = -1.0 / ((knee + fe) * ln10);
                row[2] = -fe * f.ln() / ((knee + fe) * ln10);
            }
            [offset, exp] => {
                r[i] = offset - exp * f.log10() - y[i];
                row[1] = -f.log10();
            }
            _ => unreachable!("two or three aperiodic params"),
        }
    }
}

fn gaussian_residual(freqs: &[f64], y: &[f64], p: &[f64], r: &mut [f64], j: &mut [f64]) {
    let n = p.len();
    for (i, f) in freqs.iter().enumerate() {
        let mut sum = 0.0;
        for (k, g) in p.chunks_exact(3).enumerate() {
            let (ctr, hgt, wid) = (g[0], g[1], g[2]);
            let ax = f - ctr;
            let e = (-ax * ax / (2.0 * wid * wid)).exp();
            sum += hgt * e;
            j[i * n + 3 * k] = hgt * e * ax / (wid * wid);
            j[i * n + 3 * k + 1] = e;
            j[i * n + 3 * k + 2] = hgt * e * ax * ax / (wid * wid * wid);
        }
        r[i] = sum - y[i];
    }
}

/// Solve `a x = b` for a symmetric positive matrix `a` (`n × n`, row-major) by Cholesky.
fn solve(a: &[f64], b: &[f64], n: usize) -> Option<Vec<f64>> {
    let mut l = vec![0.0; n * n];
    for i in 0..n {
        for k in 0..=i {
            let s: f64 = (0..k).map(|m| l[i * n + m] * l[k * n + m]).sum();
            if i == k {
                let d = a[i * n + i] - s;
                if d <= 0.0 || d.is_nan() {
                    return None;
                }
                l[i * n + i] = d.sqrt();
            } else {
                l[i * n + k] = (a[i * n + k] - s) / l[k * n + k];
            }
        }
    }
    let mut y = vec![0.0; n];
    for i in 0..n {
        y[i] = (b[i] - (0..i).map(|m| l[i * n + m] * y[m]).sum::<f64>()) / l[i * n + i];
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        x[i] = (y[i] - (i + 1..n).map(|m| l[m * n + i] * x[m]).sum::<f64>()) / l[i * n + i];
    }
    Some(x)
}

/// Levenberg–Marquardt least squares inside box bounds, with specparam's `curve_fit`
/// tolerances: a coordinate pressed against a bound is held there, the rest move freely.
fn least_squares(
    x0: &[f64],
    lo: &[f64],
    hi: &[f64],
    m: usize,
    residual: impl Fn(&[f64], &mut [f64], &mut [f64]),
) -> Result<Vec<f64>, String> {
    let n = x0.len();
    let clamp = |x: &mut [f64]| x.iter_mut().enumerate().for_each(|(i, v)| *v = v.clamp(lo[i], hi[i]));
    let mut x = x0.to_vec();
    clamp(&mut x);
    let (mut r, mut j) = (vec![0.0; m], vec![0.0; m * n]);
    let (mut rn, mut jn) = (vec![0.0; m], vec![0.0; m * n]);
    residual(&x, &mut r, &mut j);
    let mut cost: f64 = r.iter().map(|v| v * v).sum();
    if !cost.is_finite() {
        return Err("the fit starts outside its domain".into());
    }
    let mut lambda = 1e-3;
    let mut nfev = 1;
    loop {
        let g: Vec<f64> = (0..n).map(|k| (0..m).map(|i| j[i * n + k] * r[i]).sum()).collect();
        let free: Vec<usize> = (0..n)
            .filter(|&k| !((x[k] <= lo[k] && g[k] > 0.0) || (x[k] >= hi[k] && g[k] < 0.0)))
            .collect();
        if free.is_empty() {
            return Ok(x);
        }
        let nf = free.len();
        let mut a = vec![0.0; nf * nf];
        for (p, &kp) in free.iter().enumerate() {
            for (q, &kq) in free.iter().enumerate() {
                a[p * nf + q] = (0..m).map(|i| j[i * n + kp] * j[i * n + kq]).sum();
            }
        }
        let gf: Vec<f64> = free.iter().map(|&k| -g[k]).collect();
        loop {
            let mut damped = a.clone();
            for p in 0..nf {
                damped[p * nf + p] += lambda * a[p * nf + p].max(1e-12);
            }
            let step = match solve(&damped, &gf, nf) {
                Some(s) => s,
                None => {
                    lambda *= 10.0;
                    if lambda > 1e16 {
                        return Ok(x);
                    }
                    continue;
                }
            };
            let mut xn = x.clone();
            for (p, &k) in free.iter().enumerate() {
                xn[k] += step[p];
            }
            clamp(&mut xn);
            residual(&xn, &mut rn, &mut jn);
            nfev += 1;
            let cost_n: f64 = rn.iter().map(|v| v * v).sum();
            if cost_n.is_finite() && cost_n < cost {
                let moved: f64 = xn.iter().zip(&x).map(|(a, b)| (a - b).powi(2)).sum::<f64>().sqrt();
                let norm: f64 = x.iter().map(|v| v * v).sum::<f64>().sqrt();
                let done = cost - cost_n <= TOL * cost || moved <= TOL * (TOL + norm);
                x = xn;
                std::mem::swap(&mut r, &mut rn);
                std::mem::swap(&mut j, &mut jn);
                cost = cost_n;
                lambda = (lambda * 0.3).max(1e-15);
                if done {
                    return Ok(x);
                }
                break;
            }
            lambda *= 10.0;
            if lambda > 1e16 {
                return Ok(x);
            }
            if nfev >= MAXFEV {
                return Err(format!("no parameters found in {MAXFEV} evaluations"));
            }
        }
        if nfev >= MAXFEV {
            return Err(format!("no parameters found in {MAXFEV} evaluations"));
        }
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn norm(a: &[f64]) -> f64 {
    dot(a, a).sqrt()
}

/// `j s` for a row-major `[m, n]` matrix.
fn matvec(j: &[f64], s: &[f64], m: usize, n: usize) -> Vec<f64> {
    (0..m).map(|i| dot(&j[i * n..(i + 1) * n], s)).collect()
}

/// `jᵀ f` for a row-major `[m, n]` matrix.
fn grad(j: &[f64], f: &[f64], m: usize, n: usize) -> Vec<f64> {
    (0..n).map(|k| (0..m).map(|i| j[i * n + k] * f[i]).sum()).collect()
}

/// The thin SVD `a = u diag(s) vᵀ` of a row-major `[rows, cols]` matrix by one-sided Jacobi
/// rotations: `u` is `[rows, cols]`, `v` is `[cols, cols]`, `s` descends.
fn svd_thin(a: &[f64], rows: usize, cols: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let mut a = a.to_vec();
    let mut v = vec![0.0; cols * cols];
    for j in 0..cols {
        v[j * cols + j] = 1.0;
    }
    for _ in 0..60 {
        let mut rotated = false;
        for p in 0..cols {
            for q in p + 1..cols {
                let (mut alpha, mut beta, mut gamma) = (0.0, 0.0, 0.0);
                for i in 0..rows {
                    let (ap, aq) = (a[i * cols + p], a[i * cols + q]);
                    alpha += ap * ap;
                    beta += aq * aq;
                    gamma += ap * aq;
                }
                if gamma.abs() <= f64::EPSILON * (alpha * beta).sqrt() || gamma == 0.0 {
                    continue;
                }
                rotated = true;
                let zeta = (beta - alpha) / (2.0 * gamma);
                let t = zeta.signum() / (zeta.abs() + (1.0 + zeta * zeta).sqrt());
                let c = 1.0 / (1.0 + t * t).sqrt();
                let sn = c * t;
                for i in 0..rows {
                    let (ap, aq) = (a[i * cols + p], a[i * cols + q]);
                    a[i * cols + p] = c * ap - sn * aq;
                    a[i * cols + q] = sn * ap + c * aq;
                }
                for i in 0..cols {
                    let (vp, vq) = (v[i * cols + p], v[i * cols + q]);
                    v[i * cols + p] = c * vp - sn * vq;
                    v[i * cols + q] = sn * vp + c * vq;
                }
            }
        }
        if !rotated {
            break;
        }
    }
    let mut order: Vec<(f64, usize)> = (0..cols).map(|j| ((0..rows).map(|i| a[i * cols + j].powi(2)).sum::<f64>().sqrt(), j)).collect();
    order.sort_by(|x, y| y.0.total_cmp(&x.0));
    let (mut u, mut sv, mut vs) = (vec![0.0; rows * cols], vec![0.0; cols], vec![0.0; cols * cols]);
    for (k, (s, j)) in order.into_iter().enumerate() {
        sv[k] = s;
        for i in 0..rows {
            u[i * cols + k] = if s > 0.0 { a[i * cols + j] / s } else { 0.0 };
        }
        for i in 0..cols {
            vs[i * cols + k] = v[i * cols + j];
        }
    }
    (u, sv, vs)
}

fn next_toward(x: f64, toward: f64) -> f64 {
    if toward > x { x.next_up() } else if toward < x { x.next_down() } else { x }
}

/// scipy's `make_strictly_feasible`: a value on or past a bound moves just inside it.
fn make_strictly_feasible(x: &mut [f64], lb: &[f64], ub: &[f64], rstep: f64) {
    for i in 0..x.len() {
        let active = if rstep == 0.0 {
            if x[i] >= ub[i] { 1 } else if x[i] <= lb[i] { -1 } else { 0 }
        } else {
            let (lower, upper) = (x[i] - lb[i], ub[i] - x[i]);
            let upper_active = ub[i].is_finite() && upper <= lower.min(rstep * ub[i].abs().max(1.0));
            let lower_active = lb[i].is_finite() && lower <= upper.min(rstep * lb[i].abs().max(1.0));
            if upper_active { 1 } else if lower_active { -1 } else { 0 }
        };
        match active {
            -1 => x[i] = if rstep == 0.0 { next_toward(lb[i], ub[i]) } else { lb[i] + rstep * lb[i].abs().max(1.0) },
            1 => x[i] = if rstep == 0.0 { next_toward(ub[i], lb[i]) } else { ub[i] - rstep * ub[i].abs().max(1.0) },
            _ => {}
        }
        if x[i] < lb[i] || x[i] > ub[i] {
            x[i] = 0.5 * (lb[i] + ub[i]);
        }
    }
}

/// Coleman–Li scaling: how far each coordinate may go before the bound its gradient points at.
fn cl_scaling(x: &[f64], g: &[f64], lb: &[f64], ub: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let (mut v, mut dv) = (vec![1.0; x.len()], vec![0.0; x.len()]);
    for i in 0..x.len() {
        if g[i] < 0.0 && ub[i].is_finite() {
            v[i] = ub[i] - x[i];
            dv[i] = -1.0;
        }
        if g[i] > 0.0 && lb[i].is_finite() {
            v[i] = x[i] - lb[i];
            dv[i] = 1.0;
        }
    }
    (v, dv)
}

/// The largest stride along `s` that stays inside the bounds, and which coordinates hit one.
fn step_size_to_bound(x: &[f64], s: &[f64], lb: &[f64], ub: &[f64]) -> (f64, Vec<bool>) {
    let steps: Vec<f64> = (0..x.len())
        .map(|i| if s[i] == 0.0 { f64::INFINITY } else { ((lb[i] - x[i]) / s[i]).max((ub[i] - x[i]) / s[i]) })
        .collect();
    let min_step = steps.iter().copied().fold(f64::INFINITY, f64::min);
    (min_step, steps.iter().map(|t| *t == min_step).collect())
}

/// Where the ray `x + t s` leaves the ball of radius `delta`, as the two roots.
fn intersect_trust_region(x: &[f64], s: &[f64], delta: f64) -> Result<(f64, f64), String> {
    let a = dot(s, s);
    if a == 0.0 {
        return Err("`s` is zero".into());
    }
    let b = dot(x, s);
    let c = dot(x, x) - delta * delta;
    if c > 0.0 {
        return Err("`x` is not within the trust region".into());
    }
    let d = (b * b - a * c).sqrt();
    let q = -(b + d.copysign(b));
    let (t1, t2) = (q / a, c / q);
    Ok(if t1 < t2 { (t1, t2) } else { (t2, t1) })
}

fn evaluate_quadratic(j: &[f64], g: &[f64], s: &[f64], diag: &[f64], m: usize, n: usize) -> f64 {
    let js = matvec(j, s, m, n);
    let q = dot(&js, &js) + s.iter().zip(diag).map(|(x, d)| x * d * x).sum::<f64>();
    0.5 * q + dot(s, g)
}

fn build_quadratic_1d(j: &[f64], g: &[f64], s: &[f64], diag: &[f64], s0: Option<&[f64]>, m: usize, n: usize) -> (f64, f64, f64) {
    let v = matvec(j, s, m, n);
    let a = 0.5 * (dot(&v, &v) + s.iter().zip(diag).map(|(x, d)| x * d * x).sum::<f64>());
    let mut b = dot(g, s);
    let mut c = 0.0;
    if let Some(s0) = s0 {
        let u = matvec(j, s0, m, n);
        b += dot(&u, &v) + (0..n).map(|i| s0[i] * diag[i] * s[i]).sum::<f64>();
        c = 0.5 * dot(&u, &u) + dot(g, s0) + 0.5 * (0..n).map(|i| s0[i] * diag[i] * s0[i]).sum::<f64>();
    }
    (a, b, c)
}

fn minimize_quadratic_1d(a: f64, b: f64, lb: f64, ub: f64, c: f64) -> (f64, f64) {
    let mut t = vec![lb, ub];
    if a != 0.0 {
        let extremum = -0.5 * b / a;
        if lb < extremum && extremum < ub {
            t.push(extremum);
        }
    }
    let mut best = (t[0], t[0] * (a * t[0] + b) + c);
    for &x in &t[1..] {
        let y = x * (a * x + b) + c;
        if y < best.1 {
            best = (x, y);
        }
    }
    best
}

/// The step inside the trust region of radius `delta`, from the SVD of the scaled Jacobian.
fn solve_lsq_trust_region(n: usize, m: usize, uf: &[f64], s: &[f64], v: &[f64], delta: f64, initial_alpha: f64) -> (Vec<f64>, f64) {
    let suf: Vec<f64> = s.iter().zip(uf).map(|(a, b)| a * b).collect();
    let phi_and_derivative = |alpha: f64| {
        let denom: Vec<f64> = s.iter().map(|x| x * x + alpha).collect();
        let p_norm = (0..n).map(|i| (suf[i] / denom[i]).powi(2)).sum::<f64>().sqrt();
        let phi = p_norm - delta;
        let phi_prime = -(0..n).map(|i| suf[i] * suf[i] / denom[i].powi(3)).sum::<f64>() / p_norm;
        (phi, phi_prime)
    };
    let times_v = |w: &[f64]| -> Vec<f64> { (0..n).map(|i| -(0..n).map(|j| v[i * n + j] * w[j]).sum::<f64>()).collect() };
    let full_rank = m >= n && s[n - 1] > f64::EPSILON * m as f64 * s[0];
    if full_rank {
        let p = times_v(&(0..n).map(|j| uf[j] / s[j]).collect::<Vec<f64>>());
        if norm(&p) <= delta {
            return (p, 0.0);
        }
    }
    let mut alpha_upper = norm(&suf) / delta;
    let mut alpha_lower = if full_rank {
        let (phi, phi_prime) = phi_and_derivative(0.0);
        -phi / phi_prime
    } else {
        0.0
    };
    let mut alpha = if !full_rank && initial_alpha == 0.0 {
        (0.001 * alpha_upper).max((alpha_lower * alpha_upper).sqrt())
    } else {
        initial_alpha
    };
    for _ in 0..10 {
        if alpha < alpha_lower || alpha > alpha_upper {
            alpha = (0.001 * alpha_upper).max((alpha_lower * alpha_upper).sqrt());
        }
        let (phi, phi_prime) = phi_and_derivative(alpha);
        if phi < 0.0 {
            alpha_upper = alpha;
        }
        let ratio = phi / phi_prime;
        alpha_lower = alpha_lower.max(alpha - ratio);
        alpha -= (phi + delta) * ratio / delta;
        if phi.abs() < 0.01 * delta {
            break;
        }
    }
    let mut p = times_v(&(0..n).map(|j| suf[j] / (s[j] * s[j] + alpha)).collect::<Vec<f64>>());
    let scale = delta / norm(&p);
    p.iter_mut().for_each(|x| *x *= scale);
    (p, alpha)
}

/// One TRF iteration's scaled problem: the Jacobian and gradient in the scaled space, the
/// scaling itself, the trust radius, the bounds and how close to them a step may land.
struct Scaled<'a> {
    j_h: &'a [f64],
    diag_h: &'a [f64],
    g_h: &'a [f64],
    d: &'a [f64],
    delta: f64,
    theta: f64,
    lb: &'a [f64],
    ub: &'a [f64],
    m: usize,
    n: usize,
}

/// scipy's `select_step`: the trust-region step, its reflection off a bound, or the scaled
/// gradient, whichever the quadratic model rates lowest, all kept strictly inside the bounds.
fn select_step(x: &[f64], t: &Scaled<'_>, mut p: Vec<f64>, mut p_h: Vec<f64>) -> Result<(Vec<f64>, Vec<f64>, f64), String> {
    let Scaled { j_h, diag_h, g_h, d, delta, theta, lb, ub, m, n } = *t;
    let within: Vec<f64> = (0..n).map(|i| x[i] + p[i]).collect();
    if (0..n).all(|i| within[i] >= lb[i] && within[i] <= ub[i]) {
        let value = evaluate_quadratic(j_h, g_h, &p_h, diag_h, m, n);
        return Ok((p, p_h, -value));
    }
    let (p_stride, hits) = step_size_to_bound(x, &p, lb, ub);
    let mut r_h = p_h.clone();
    for i in 0..n {
        if hits[i] && p[i] != 0.0 {
            r_h[i] = -r_h[i];
        }
    }
    let mut r: Vec<f64> = (0..n).map(|i| d[i] * r_h[i]).collect();
    p.iter_mut().for_each(|v| *v *= p_stride);
    p_h.iter_mut().for_each(|v| *v *= p_stride);
    let x_on_bound: Vec<f64> = (0..n).map(|i| x[i] + p[i]).collect();
    let (_, to_tr) = intersect_trust_region(&p_h, &r_h, delta)?;
    let (to_bound, _) = step_size_to_bound(&x_on_bound, &r, lb, ub);
    let r_stride = to_bound.min(to_tr);
    let (r_stride_l, r_stride_u) = if r_stride > 0.0 {
        ((1.0 - theta) * p_stride / r_stride, if r_stride == to_bound { theta * to_bound } else { to_tr })
    } else {
        (0.0, -1.0)
    };
    let r_value = if r_stride_l <= r_stride_u {
        let (a, b, c) = build_quadratic_1d(j_h, g_h, &r_h, diag_h, Some(&p_h), m, n);
        let (stride, value) = minimize_quadratic_1d(a, b, r_stride_l, r_stride_u, c);
        for i in 0..n {
            r_h[i] = r_h[i] * stride + p_h[i];
            r[i] = r_h[i] * d[i];
        }
        value
    } else {
        f64::INFINITY
    };
    p.iter_mut().for_each(|v| *v *= theta);
    p_h.iter_mut().for_each(|v| *v *= theta);
    let p_value = evaluate_quadratic(j_h, g_h, &p_h, diag_h, m, n);
    let mut ag_h: Vec<f64> = g_h.iter().map(|v| -v).collect();
    let mut ag: Vec<f64> = (0..n).map(|i| d[i] * ag_h[i]).collect();
    let to_tr = delta / norm(&ag_h);
    let (to_bound, _) = step_size_to_bound(x, &ag, lb, ub);
    let ag_stride = if to_bound < to_tr { theta * to_bound } else { to_tr };
    let (a, b, _) = build_quadratic_1d(j_h, g_h, &ag_h, diag_h, None, m, n);
    let (ag_stride, ag_value) = minimize_quadratic_1d(a, b, 0.0, ag_stride, 0.0);
    ag_h.iter_mut().for_each(|v| *v *= ag_stride);
    ag.iter_mut().for_each(|v| *v *= ag_stride);
    Ok(if p_value < r_value && p_value < ag_value {
        (p, p_h, -p_value)
    } else if r_value < p_value && r_value < ag_value {
        (r, r_h, -r_value)
    } else {
        (ag, ag_h, -ag_value)
    })
}

/// scipy's bounded trust-region-reflective least squares, as `curve_fit` runs it for a bounded
/// problem: `x_scale` 1, the exact trust-region solver and specparam's tolerances.
fn trf(x0: &[f64], lb: &[f64], ub: &[f64], m: usize, residual: impl Fn(&[f64], &mut [f64], &mut [f64])) -> Result<Vec<f64>, String> {
    let n = x0.len();
    let mut x = x0.to_vec();
    make_strictly_feasible(&mut x, lb, ub, 1e-10);
    let (mut f, mut jac) = (vec![0.0; m], vec![0.0; m * n]);
    residual(&x, &mut f, &mut jac);
    let mut nfev = 1;
    let mut cost = 0.5 * dot(&f, &f);
    let mut g = grad(&jac, &f, m, n);
    let (v, _) = cl_scaling(&x, &g, lb, ub);
    let mut delta = norm(&(0..n).map(|i| x[i] / v[i].sqrt()).collect::<Vec<f64>>());
    if delta == 0.0 {
        delta = 1.0;
    }
    let mut alpha = 0.0;
    let mut status: Option<u8> = None;
    let (mut x_new, mut f_new, mut j_new) = (x.clone(), vec![0.0; m], vec![0.0; m * n]);
    loop {
        let (v, dv) = cl_scaling(&x, &g, lb, ub);
        let g_norm = (0..n).map(|i| (g[i] * v[i]).abs()).fold(0.0, f64::max);
        if g_norm < TOL {
            status = Some(1);
        }
        if status.is_some() || nfev == MAXFEV {
            break;
        }
        let d: Vec<f64> = v.iter().map(|x| x.sqrt()).collect();
        let diag_h: Vec<f64> = (0..n).map(|i| g[i] * dv[i]).collect();
        let g_h: Vec<f64> = (0..n).map(|i| d[i] * g[i]).collect();
        let rows = m + n;
        let mut ja = vec![0.0; rows * n];
        for i in 0..m {
            for k in 0..n {
                ja[i * n + k] = jac[i * n + k] * d[k];
            }
        }
        for k in 0..n {
            ja[(m + k) * n + k] = diag_h[k].sqrt();
        }
        let (u, s, vv) = svd_thin(&ja, rows, n);
        let uf: Vec<f64> = (0..n).map(|k| (0..m).map(|i| u[i * n + k] * f[i]).sum()).collect();
        let j_h = &ja[..m * n];
        let theta = 0.995f64.max(1.0 - g_norm);
        let mut actual_reduction = -1.0;
        let mut cost_new = cost;
        while actual_reduction <= 0.0 && nfev < MAXFEV {
            let (p_h, a) = solve_lsq_trust_region(n, m, &uf, &s, &vv, delta, alpha);
            alpha = a;
            let p: Vec<f64> = (0..n).map(|i| d[i] * p_h[i]).collect();
            let scaled = Scaled { j_h, diag_h: &diag_h, g_h: &g_h, d: &d, delta, theta, lb, ub, m, n };
            let (step, step_h, predicted) = select_step(&x, &scaled, p, p_h)?;
            for i in 0..n {
                x_new[i] = x[i] + step[i];
            }
            make_strictly_feasible(&mut x_new, lb, ub, 0.0);
            residual(&x_new, &mut f_new, &mut j_new);
            nfev += 1;
            let step_h_norm = norm(&step_h);
            if !f_new.iter().all(|v| v.is_finite()) {
                delta = 0.25 * step_h_norm;
                continue;
            }
            cost_new = 0.5 * dot(&f_new, &f_new);
            actual_reduction = cost - cost_new;
            let ratio = if predicted > 0.0 {
                actual_reduction / predicted
            } else if predicted == 0.0 && actual_reduction == 0.0 {
                1.0
            } else {
                0.0
            };
            let delta_new = if ratio < 0.25 {
                0.25 * step_h_norm
            } else if ratio > 0.75 && step_h_norm > 0.95 * delta {
                delta * 2.0
            } else {
                delta
            };
            let ftol_met = actual_reduction < TOL * cost && ratio > 0.25;
            let xtol_met = norm(&step) < TOL * (TOL + norm(&x));
            status = match (ftol_met, xtol_met) {
                (true, true) => Some(4),
                (true, false) => Some(2),
                (false, true) => Some(3),
                _ => None,
            };
            if status.is_some() {
                break;
            }
            alpha *= delta / delta_new;
            delta = delta_new;
        }
        if actual_reduction > 0.0 {
            std::mem::swap(&mut x, &mut x_new);
            std::mem::swap(&mut f, &mut f_new);
            std::mem::swap(&mut jac, &mut j_new);
            cost = cost_new;
            g = grad(&jac, &f, m, n);
        }
    }
    match status {
        Some(_) => Ok(x),
        None => Err(format!("no parameters found in {MAXFEV} evaluations")),
    }
}

fn ap_guess(freqs: &[f64], y: &[f64], knee: bool) -> Vec<f64> {
    let exp = ((y[y.len() - 1] - y[0]) / (freqs[freqs.len() - 1].log10() - freqs[0].log10())).abs();
    if knee { vec![y[0], 0.0, exp] } else { vec![y[0], exp] }
}

fn simple_ap_fit(freqs: &[f64], y: &[f64], guess: Vec<f64>) -> Result<Vec<f64>, String> {
    let n = guess.len();
    least_squares(&guess, &vec![f64::NEG_INFINITY; n], &vec![f64::INFINITY; n], freqs.len(), |p, r, j| {
        aperiodic_residual(freqs, y, p, r, j)
    })
    .map_err(|e| format!("aperiodic fit: {e}"))
}

/// numpy's `percentile`: linear interpolation between the sorted neighbours of the rank.
fn percentile(values: &[f64], q: f64) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let pos = q / 100.0 * (v.len() - 1) as f64;
    let (low, frac) = (pos.floor() as usize, pos.fract());
    v[low] + (v[(low + 1).min(v.len() - 1)] - v[low]) * frac
}

fn robust_ap_fit(freqs: &[f64], y: &[f64], knee: bool) -> Result<Vec<f64>, String> {
    let popt = simple_ap_fit(freqs, y, ap_guess(freqs, y, knee))?;
    let flat: Vec<f64> = y.iter().zip(aperiodic(freqs, &popt)).map(|(p, a)| (p - a).max(0.0)).collect();
    let thresh = percentile(&flat, AP_PERCENTILE_THRESH);
    let kept: Vec<usize> = (0..y.len()).filter(|&i| flat[i] <= thresh).collect();
    let f: Vec<f64> = kept.iter().map(|&i| freqs[i]).collect();
    let s: Vec<f64> = kept.iter().map(|&i| y[i]).collect();
    simple_ap_fit(&f, &s, popt)
}

/// Full width at half maximum from the shorter side of the peak at `peak`, in Hz; `None` when
/// neither side drops to half.
fn estimate_fwhm(flat: &[f64], peak: usize, freq_res: f64) -> Option<f64> {
    let half = 0.5 * flat[peak];
    let left = (1..peak).rev().find(|&i| flat[i] <= half).map(|i| peak - i);
    let right = (peak + 1..flat.len()).find(|&i| flat[i] <= half).map(|i| i - peak);
    [left, right].into_iter().flatten().min().map(|short| short as f64 * 2.0 * freq_res)
}

fn std(v: &[f64]) -> f64 {
    let n = v.len() as f64;
    let mean = v.iter().sum::<f64>() / n;
    (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt()
}

fn fit_peaks(freqs: &[f64], flatspec: &[f64], s: &Settings) -> Result<Vec<[f64; 3]>, String> {
    let freq_res = freqs[1] - freqs[0];
    let range = (freqs[0], freqs[freqs.len() - 1]);
    let (wl0, wl1) = s.peak_width_limits;
    let mut flat = flatspec.to_vec();
    let mut guess: Vec<[f64; 3]> = Vec::new();
    while guess.len() < s.max_n_peaks {
        let max_ind = (0..flat.len()).max_by(|&a, &b| flat[a].total_cmp(&flat[b])).expect("a bin");
        let max_height = flat[max_ind];
        if max_height <= s.peak_threshold * std(&flat) || max_height <= s.min_peak_height || max_height.is_nan() {
            break;
        }
        let mut guess_std = match estimate_fwhm(&flat, max_ind, freq_res) {
            Some(fwhm) => fwhm / (2.0 * (2.0 * std::f64::consts::LN_2).sqrt()),
            None => (wl0 + wl1) / 2.0,
        };
        if guess_std < wl0 / 2.0 {
            guess_std = wl0 / 2.0;
        }
        // specparam's own rule: a guess wider than the limit restarts at the narrowest.
        if guess_std > wl1 / 2.0 {
            guess_std = wl0 / 2.0;
        }
        let cur = [freqs[max_ind], max_height, guess_std];
        guess.push(cur);
        for (f, g) in flat.iter_mut().zip(gaussians(freqs, &cur)) {
            *f -= g;
        }
    }
    // Drop a peak whose centre sits within a width of either edge…
    guess.retain(|g| (g[0] - range.0).abs() > g[2] * BW_STD_EDGE && (g[0] - range.1).abs() > g[2] * BW_STD_EDGE);
    // …and of two that overlap, the lower.
    guess.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut drop = vec![false; guess.len()];
    for i in 0..guess.len().saturating_sub(1) {
        let (a, b) = (guess[i], guess[i + 1]);
        if a[0] + a[2] * GAUSS_OVERLAP_THRESH > b[0] - b[2] * GAUSS_OVERLAP_THRESH {
            drop[if b[1] < a[1] { i + 1 } else { i }] = true;
        }
    }
    let guess: Vec<[f64; 3]> = guess.into_iter().zip(drop).filter(|(_, d)| !d).map(|(g, _)| g).collect();
    if guess.is_empty() {
        return Ok(Vec::new());
    }
    let (mut lo, mut hi, mut x0) = (Vec::new(), Vec::new(), Vec::new());
    for g in &guess {
        lo.extend([(g[0] - 2.0 * CF_BOUND * g[2]).max(range.0), 0.0, wl0 / 2.0]);
        hi.extend([(g[0] + 2.0 * CF_BOUND * g[2]).min(range.1), f64::INFINITY, wl1 / 2.0]);
        x0.extend(*g);
    }
    let fitted = trf(&x0, &lo, &hi, freqs.len(), |p, r, j| gaussian_residual(freqs, flatspec, p, r, j))
        .map_err(|e| format!("peak fit: {e}"))?;
    let mut peaks: Vec<[f64; 3]> = fitted.chunks_exact(3).map(|g| [g[0], g[1], g[2]]).collect();
    peaks.sort_by(|a, b| a[0].total_cmp(&b[0]));
    Ok(peaks)
}

/// specparam's `fit` on one spectrum: `freqs` already trimmed and `powers` in log10.
pub fn fit_spectrum(freqs: &[f64], powers: &[f64], s: &Settings) -> Result<Fit, String> {
    if freqs.len() < 3 {
        return Err("needs at least three bins in the frequency range".into());
    }
    let temp = robust_ap_fit(freqs, powers, s.knee)?;
    let temp_fit = aperiodic(freqs, &temp);
    let flat: Vec<f64> = powers.iter().zip(&temp_fit).map(|(p, a)| p - a).collect();
    let gaussians_fit = fit_peaks(freqs, &flat, s)?;
    let peak_fit = gaussians(freqs, &gaussians_fit.concat());
    let peak_rm: Vec<f64> = powers.iter().zip(&peak_fit).map(|(p, g)| p - g).collect();
    let ap = simple_ap_fit(freqs, &peak_rm, ap_guess(freqs, &peak_rm, s.knee))?;
    let ap_fit = aperiodic(freqs, &ap);
    Ok(Fit { aperiodic: ap, gaussians: gaussians_fit, ap_fit, peak_fit })
}

#[derive(Default)]
struct Specparam;

impl Node for Specparam {
    fn process(
        &mut self,
        inp: &Inputs<'_>,
        out: &mut Outputs<'_>,
        _c: &mut NodeCtx,
        p: &Params<'_>,
    ) -> NodeResult {
        let d = inp.get("psd").ok_or("`psd` is required")?;
        let a = d.assert_ndims().at_least(1)?;
        let dim = a.shape().len() - 1;
        let freqs: Vec<f64> = d
            .meta()
            .channels()
            .get(dim)
            .and_then(|axis| axis.coords.as_ref())
            .map(|c| c.iter().filter_map(|c| if let Coord::Num(f) = c { Some(*f) } else { None }).collect())
            .filter(|f: &Vec<f64>| f.len() == a.shape()[dim])
            .ok_or("needs the frequencies on its last axis, as a `Psd` frame carries them")?;
        let s = Settings {
            knee: p.str("specparam", "mode").unwrap_or("fixed") == "knee",
            peak_width_limits: (p.f64("specparam", "peak_width_min").unwrap_or(0.5), p.f64("specparam", "peak_width_max").unwrap_or(12.0)),
            max_n_peaks: p.f64("specparam", "max_peaks").unwrap_or(6.0) as usize,
            min_peak_height: 0.0,
            peak_threshold: p.f64("specparam", "peak_threshold").unwrap_or(2.0),
        };
        let range = (p.f64("specparam", "freq_min").unwrap_or(1.0), p.f64("specparam", "freq_max").unwrap_or(30.0));
        let kept = bins_within(&freqs, range);
        if kept.len() < 3 {
            return Err(format!("fewer than three bins fall between {} Hz and {} Hz", range.0, range.1).into());
        }
        let fit_freqs: Vec<f64> = kept.iter().map(|&i| freqs[i]).collect();

        let lanes = stream::lanes(a.shape(), dim, a.as_bytes());
        let (mut offset, mut exponent) = (Vec::with_capacity(lanes.len()), Vec::with_capacity(lanes.len()));
        let (mut ap_fits, mut peak_fits) = (Vec::with_capacity(lanes.len()), Vec::with_capacity(lanes.len()));
        for lane in &lanes {
            let powers: Vec<f64> = kept.iter().map(|&i| (lane[i] as f64).log10()).collect();
            // A spectrum the model cannot fit answers NaN rather than stopping the stream.
            match fit_spectrum(&fit_freqs, &powers, &s) {
                Ok(fit) => {
                    offset.push(fit.aperiodic[0] as f32);
                    exponent.push(fit.aperiodic[fit.aperiodic.len() - 1] as f32);
                    ap_fits.push(fit.ap_fit.iter().map(|v| *v as f32).collect::<Vec<f32>>());
                    peak_fits.push(fit.peak_fit.iter().map(|v| *v as f32).collect::<Vec<f32>>());
                }
                Err(_) => {
                    offset.push(f32::NAN);
                    exponent.push(f32::NAN);
                    ap_fits.push(vec![f32::NAN; kept.len()]);
                    peak_fits.push(vec![f32::NAN; kept.len()]);
                }
            }
        }

        let lead: Vec<usize> = a.shape()[..dim].to_vec();
        let scalar_shape = if lead.is_empty() { vec![1] } else { lead.clone() };
        let scalar_meta = d.meta().clone().with_channels(Axes(d.meta().channels().0.iter().take(dim).cloned().collect())).with_sfreq(None);
        let mut shape_out = a.shape().to_vec();
        shape_out[dim] = kept.len();
        let component_meta = d.meta().keep(dim, &kept).with_sfreq(None);
        let bytes = |v: &[f32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
        out.set("offset", Data::array_f32(scalar_shape.clone(), bytes(&offset), scalar_meta.clone()).map_err(|e| e.to_string())?);
        out.set("exponent", Data::array_f32(scalar_shape, bytes(&exponent), scalar_meta).map_err(|e| e.to_string())?);
        out.set("aperiodic", Data::array_f32(shape_out.clone(), stream::unlanes(&shape_out, dim, &ap_fits), component_meta.clone()).map_err(|e| e.to_string())?);
        out.set("periodic", Data::array_f32(shape_out.clone(), stream::unlanes(&shape_out, dim, &peak_fits), component_meta).map_err(|e| e.to_string())?);
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "specparam",
        name: "mode",
        spec: ParamSpec::Str { default: "fixed", options: &["fixed", "knee"], refresh: false },
        expression: None,
        doc: Some("`knee` adds a bend where the 1/f slope flattens at low frequency; the `aperiodic` component carries it."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "specparam",
        name: "freq_min",
        spec: ParamSpec::Num { default: &[1.0], min: 0.0, max: 1000.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Lowest frequency fitted, in Hz."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "specparam",
        name: "freq_max",
        spec: ParamSpec::Num { default: &[30.0], min: 0.0, max: 1000.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Highest frequency fitted, in Hz."),
        section: 0,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "specparam",
        name: "max_peaks",
        spec: ParamSpec::Num { default: &[6.0], min: 1.0, max: 20.0, int: true, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Peaks to fit at most."),
        section: 1,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "specparam",
        name: "peak_threshold",
        spec: ParamSpec::Num { default: &[2.0], min: 0.0, max: 10.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("How far a peak must rise over the flattened spectrum to count, in standard deviations of it."),
        section: 1,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "specparam",
        name: "peak_width_min",
        spec: ParamSpec::Num { default: &[0.5], min: 0.0, max: 100.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Narrowest peak fitted, in Hz."),
        section: 1,
        show: &[],
        role: None,
    },
    ParamDecl {
        group: "specparam",
        name: "peak_width_max",
        spec: ParamSpec::Num { default: &[12.0], min: 0.0, max: 512.0, int: false, options: &[], face: Face::Plain },
        expression: None,
        doc: Some("Widest peak fitted, in Hz."),
        section: 1,
        show: &[],
        role: None,
    },
];
static INPUTS: &[SlotDecl] = &[SlotDecl { name: "psd", kind: SlotType::Array, trigger_process: true, multi: false, required: true }];
static OUTPUTS: &[OutputDecl] = &[
    OutputDecl { name: "offset", kind: SlotType::Array },
    OutputDecl { name: "exponent", kind: SlotType::Array },
    OutputDecl { name: "aperiodic", kind: SlotType::Array },
    OutputDecl { name: "periodic", kind: SlotType::Array },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Analysis, Tag::Eeg],
    doc: "The aperiodic 1/f offset and exponent of a spectrum, and its aperiodic and periodic components, per channel.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(Specparam, MANIFEST);
