//! The easing curves a transition travels by: one owner, so every ramp in the tree agrees.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How progress along a transition maps to the blend between where it started and its target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
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
}

impl Curve {
    pub const ALL: [Curve; 6] = [Curve::Step, Curve::Linear, Curve::In, Curve::Out, Curve::InOut, Curve::Smooth];

    pub fn as_str(self) -> &'static str {
        match self {
            Curve::Step => "step",
            Curve::Linear => "linear",
            Curve::In => "in",
            Curve::Out => "out",
            Curve::InOut => "in_out",
            Curve::Smooth => "smooth",
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
        }
    }
}
