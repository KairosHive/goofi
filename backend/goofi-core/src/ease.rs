//! The easing curves an attribute travels by on a transition, and a track's value from a keyframe
//! to the next: one owner, so every ramp in the tree agrees — the Rust blend a condition reads
//! mid-flight and the Python a functional runs.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How progress along a transition maps to the blend between where it started and its target:
/// a preset, or a line of anchor points a hand placed, each joined to the next by a cubic bezier.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Curve {
    /// A keyframe's curve alone: the value goes through the keyframe at the slope the keyframes
    /// beside it give it, so a run that falls keeps falling. What knows those keyframes makes
    /// it a curve of anchor points (`goofi_graph::timeline::shape`); with none beside it, as on
    /// a transition, it is `linear`.
    Smooth,
    /// Everything switches on arrival.
    Step,
    #[default]
    Linear,
    /// Slow away from the start and slow into the target: level at both ends.
    Easing,
    /// The anchor points from (0, 0) to (1, 1), each `[x, y, in_dx, in_dy, out_dx, out_dy]`: where
    /// it is, and its two handles as offsets from it. A handle's x stays inside the segment it
    /// shapes; its y is free to overshoot.
    Custom(Vec<[f64; 6]>),
}

/// How many points the Python form of a custom curve samples: linear between them, a fraction of
/// a percent off the exact curve at worst.
const SAMPLES: usize = 129;

impl Curve {
    /// The presets that stand alone, with no keyframes beside them.
    pub const PRESETS: [Curve; 3] = [Curve::Step, Curve::Linear, Curve::Easing];

    /// The preset's name; a curve of anchor points is `custom`.
    pub fn as_str(&self) -> &'static str {
        match self {
            Curve::Smooth => "smooth",
            Curve::Step => "step",
            Curve::Linear => "linear",
            Curve::Easing => "easing",
            Curve::Custom(_) => "custom",
        }
    }

    /// Why the curve cannot be travelled: it runs from (0, 0) to (1, 1) through points each
    /// further along than the last, and a handle that leaves its segment would fold the curve
    /// back on itself, so one progress would have two blends.
    pub fn check(&self) -> Result<(), String> {
        let Curve::Custom(points) = self else { return Ok(()) };
        let ends = points.len() >= 2 && points[0][..2] == [0.0, 0.0] && points[points.len() - 1][..2] == [1.0, 1.0];
        let whole = points.iter().flatten().all(|v| v.is_finite());
        let forward = points.windows(2).all(|w| {
            let span = w[1][0] - w[0][0];
            span > 0.0 && (0.0..=span).contains(&w[0][4]) && (-span..=0.0).contains(&w[1][2])
        });
        (ends && whole && forward).then_some(()).ok_or_else(|| {
            "a custom curve is points `[x, y, in_dx, in_dy, out_dx, out_dy]` from (0, 0) to (1, 1), each further along than the last, each handle's x inside its segment".to_string()
        })
    }

    /// The blend at progress `x` in 0..1: 0 is where the move began, 1 is the target.
    pub fn at(&self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Curve::Step => if x >= 1.0 { 1.0 } else { 0.0 },
            Curve::Smooth | Curve::Linear => x,
            Curve::Easing => if x < 0.5 { 2.0 * x * x } else { 1.0 - 2.0 * (1.0 - x) * (1.0 - x) },
            Curve::Custom(points) => {
                let Some(i) = points.windows(2).position(|w| x <= w[1][0]) else { return x };
                let (a, b) = (points[i], points[i + 1]);
                let xs = [a[0], a[0] + a[4], b[0] + b[2], b[0]];
                // The handles' x inside the segment keep x(t) monotonic, so a bisection finds the one t.
                let (mut lo, mut hi) = (0.0, 1.0);
                for _ in 0..48 {
                    let t = (lo + hi) / 2.0;
                    if cubic(xs, t) < x { lo = t } else { hi = t }
                }
                cubic([a[1], a[1] + a[5], b[1] + b[3], b[1]], (lo + hi) / 2.0)
            }
        }
    }

    /// [`Self::at`] as Python over `x`, for a functional a reader runs in the interpreter: a
    /// custom curve as `np.interp` over its samples, but for one bezier whose handles stand a
    /// third and two thirds along, which is a cubic in `x` itself, as a recording fits them.
    pub fn python(&self) -> String {
        match self {
            Curve::Custom(points) if matches!(points[..], [a, b] if a[4] == 1.0 / 3.0 && b[2] == -1.0 / 3.0) => {
                format!("(3.0 * (1.0 - x) * (1.0 - x) * x * {:?} + 3.0 * (1.0 - x) * x * x * {:?} + x * x * x)", points[0][5], 1.0 + points[1][3])
            }
            Curve::Step => "(1.0 if x >= 1.0 else 0.0)".into(),
            Curve::Smooth | Curve::Linear => "x".into(),
            Curve::Easing => "(2.0 * x * x if x < 0.5 else 1.0 - 2.0 * (1.0 - x) * (1.0 - x))".into(),
            Curve::Custom(_) => {
                let xs: Vec<String> = (0..SAMPLES).map(|i| format!("{:?}", i as f64 / (SAMPLES - 1) as f64)).collect();
                let ys: Vec<String> = (0..SAMPLES).map(|i| format!("{:?}", self.at(i as f64 / (SAMPLES - 1) as f64))).collect();
                format!("float(np.interp(x, [{}], [{}]))", xs.join(", "), ys.join(", "))
            }
        }
    }
}

/// One coordinate of the cubic bezier through the four values `p`, at parameter `t`.
fn cubic(p: [f64; 4], t: f64) -> f64 {
    let u = 1.0 - t;
    u * u * u * p[0] + 3.0 * u * u * t * p[1] + 3.0 * u * t * t * p[2] + t * t * t * p[3]
}
