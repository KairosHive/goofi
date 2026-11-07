//! The easing curves an attribute travels by on a transition: one owner, so every ramp in the
//! tree agrees — the Rust blend a condition reads mid-flight and the Python a functional runs.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How progress along a transition maps to the blend between where it started and its target:
/// a preset, or a cubic bezier from (0, 0) through two handles to (1, 1), as CSS writes one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Curve {
    /// Everything switches on arrival.
    Step,
    #[default]
    Linear,
    In,
    Out,
    InOut,
    Smooth,
    /// `[x1, y1, x2, y2]`: the handles' x in 0..1, the y free to overshoot.
    Bezier([f64; 4]),
}

/// How many points the Python form of a bezier samples: linear between them, a fraction of a
/// percent off the exact curve at worst.
const SAMPLES: usize = 129;

impl Curve {
    pub const PRESETS: [Curve; 6] = [Curve::Step, Curve::Linear, Curve::In, Curve::Out, Curve::InOut, Curve::Smooth];

    /// The preset's name; a bezier is `bezier`.
    pub fn as_str(self) -> &'static str {
        match self {
            Curve::Step => "step",
            Curve::Linear => "linear",
            Curve::In => "in",
            Curve::Out => "out",
            Curve::InOut => "in_out",
            Curve::Smooth => "smooth",
            Curve::Bezier(_) => "bezier",
        }
    }

    /// Why the curve cannot be travelled: a bezier handle's x outside 0..1 folds the curve back
    /// on itself, so one progress would have two blends.
    pub fn check(self) -> Result<(), String> {
        match self {
            Curve::Bezier([x1, _, x2, _]) if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) || !x1.is_finite() || !x2.is_finite() => {
                Err(format!("a bezier's handles have x in 0..1, not {x1} and {x2}"))
            }
            Curve::Bezier(h) if h.iter().any(|v| !v.is_finite()) => Err("a bezier's handles are four finite numbers".into()),
            _ => Ok(()),
        }
    }

    /// The blend at progress `x` in 0..1: 0 is where the move began, 1 is the target.
    pub fn at(self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Curve::Step => if x >= 1.0 { 1.0 } else { 0.0 },
            Curve::Linear => x,
            Curve::In => x * x,
            Curve::Out => 1.0 - (1.0 - x) * (1.0 - x),
            Curve::InOut => if x < 0.5 { 2.0 * x * x } else { 1.0 - 2.0 * (1.0 - x) * (1.0 - x) },
            Curve::Smooth => x * x * (3.0 - 2.0 * x),
            Curve::Bezier(h) => bezier_y(h, bezier_t(h, x)),
        }
    }

    /// [`Self::at`] as Python over `x`, for a functional a reader runs in the interpreter: a
    /// bezier as `np.interp` over its samples.
    pub fn python(self) -> String {
        match self {
            Curve::Step => "(1.0 if x >= 1.0 else 0.0)".into(),
            Curve::Linear => "x".into(),
            Curve::In => "x * x".into(),
            Curve::Out => "1.0 - (1.0 - x) * (1.0 - x)".into(),
            Curve::InOut => "(2.0 * x * x if x < 0.5 else 1.0 - 2.0 * (1.0 - x) * (1.0 - x))".into(),
            Curve::Smooth => "x * x * (3.0 - 2.0 * x)".into(),
            Curve::Bezier(_) => {
                let xs: Vec<String> = (0..SAMPLES).map(|i| format!("{:?}", i as f64 / (SAMPLES - 1) as f64)).collect();
                let ys: Vec<String> = (0..SAMPLES).map(|i| format!("{:?}", self.at(i as f64 / (SAMPLES - 1) as f64))).collect();
                format!("float(np.interp(x, [{}], [{}]))", xs.join(", "), ys.join(", "))
            }
        }
    }
}

/// One coordinate of the cubic from 0 through `a` and `b` to 1, at parameter `t`.
fn cubic(a: f64, b: f64, t: f64) -> f64 {
    let u = 1.0 - t;
    3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
}

fn bezier_y([_, y1, _, y2]: [f64; 4], t: f64) -> f64 {
    cubic(y1, y2, t)
}

/// The parameter at which the curve's x reaches `x`: Newton from the diagonal, bisection where
/// the slope is too flat for it. The handles' x in 0..1 keep x(t) monotonic, so there is one.
fn bezier_t([x1, _, x2, _]: [f64; 4], x: f64) -> f64 {
    let mut t = x;
    for _ in 0..8 {
        let dx = cubic(x1, x2, t) - x;
        let u = 1.0 - t;
        let slope = 3.0 * u * u * x1 + 6.0 * u * t * (x2 - x1) + 3.0 * t * t * (1.0 - x2);
        if dx.abs() < 1e-7 {
            return t;
        }
        if slope.abs() < 1e-6 {
            break;
        }
        t = (t - dx / slope).clamp(0.0, 1.0);
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..40 {
        t = (lo + hi) / 2.0;
        if cubic(x1, x2, t) < x {
            lo = t;
        } else {
            hi = t;
        }
    }
    t
}
