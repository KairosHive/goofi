//! The universal `common` scheduling group and the policy a host node reads off it.

use goofi_core::Param;
use goofi_node::{param, ExprDecl, ExprMode, NodeManifest, ParamDecl, ParamGroups, ParamSpec};

use crate::COMMON;

/// The two ways a user can author `common.max_frequency`; [`RunPolicy`] normalizes both to Hz.
pub const FREQ_MODE_UPDATES_PER_SECOND: &str = "updates-per-second";
pub const FREQ_MODE_SECONDS_PER_UPDATE: &str = "seconds-per-update";

/// When a node's `process` may run, lifted out of the params.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct RunPolicy {
    /// Run whenever the rate cap allows, with no fresh input — a free-running producer.
    pub autotrigger: bool,
    /// Max run rate in Hz; `<= 0` is unbounded. Seconds-per-update is normalized to a rate here.
    pub max_frequency: f64,
}

impl RunPolicy {
    /// The minimum seconds between runs, or `None` when unbounded.
    pub fn period(&self) -> Option<f64> {
        (self.max_frequency > 0.0).then(|| 1.0 / self.max_frequency)
    }

    /// Read the policy from a node's `common` param group, defaulting each absent field.
    pub fn from_params(p: &ParamGroups) -> RunPolicy {
        let autotrigger = param(p, COMMON, "autotrigger").and_then(Param::as_bool).unwrap_or(false);
        let raw = param(p, COMMON, "max_frequency").and_then(Param::as_f64).unwrap_or(0.0);
        let seconds_per_update =
            param(p, COMMON, "frequency_mode").and_then(Param::as_str) == Some(FREQ_MODE_SECONDS_PER_UPDATE);
        let max_frequency = if seconds_per_update && raw > 0.0 { 1.0 / raw } else { raw };
        RunPolicy { autotrigger, max_frequency }
    }
}

/// The universal `common` scheduling group; a fourth param is added here and nowhere else. It may
/// read the manifest's static shape, but never `m.params` for a `common` key.
pub fn common_decls(m: &NodeManifest) -> [ParamDecl; 3] {
    let decl = |name, spec, expression, doc| ParamDecl { group: COMMON, name, spec, expression, doc: Some(doc), section: 0, show: None, role: None };
    [
        decl(
            "autotrigger",
            ParamSpec::Bool { default: m.producer },
            None,
            "Run on the node's own schedule, instead of waiting for an input frame. \
             Turn this on for sources; leave it off for transforms driven by their input.",
        ),
        // Live on a producer; `trigger: true` is inert, as a `common.*` arrival never triggers a run.
        decl(
            "max_frequency",
            ParamSpec::Float { default: 0.0, min: 0.0, max: 100.0 },
            Some(ExprDecl {
                source: "variables.system.default_ufreq",
                mode: if m.producer { ExprMode::On } else { ExprMode::Off },
                trigger: true,
            }),
            "Rate cap for this node, read through `frequency_mode`. 0 means uncapped — the node \
             runs as often as the scheduler and its inputs allow.",
        ),
        decl(
            "frequency_mode",
            ParamSpec::Str {
                default: FREQ_MODE_UPDATES_PER_SECOND,
                options: &[FREQ_MODE_UPDATES_PER_SECOND, FREQ_MODE_SECONDS_PER_UPDATE],
                refresh: false,
            },
            None,
            "How to read `max_frequency`: as a rate in Hz (updates per second), or as a period \
             in seconds between updates — convenient for very slow nodes.",
        ),
    ]
}
