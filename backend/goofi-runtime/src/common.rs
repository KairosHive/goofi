//! The universal `common` scheduling group and the policy a host node reads off it.

use goofi_core::Param;
use goofi_node::{param, ExprDecl, ExprMode, NodeManifest, ParamDecl, ParamGroups, ParamSpec};

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
        let autotrigger = param(p, "common", "autotrigger").and_then(Param::as_bool).unwrap_or(false);
        let raw = param(p, "common", "max_frequency").and_then(Param::as_f64).unwrap_or(0.0);
        let seconds_per_update =
            param(p, "common", "frequency_mode").and_then(Param::as_str) == Some(FREQ_MODE_SECONDS_PER_UPDATE);
        let max_frequency = if seconds_per_update && raw > 0.0 { 1.0 / raw } else { raw };
        RunPolicy { autotrigger, max_frequency }
    }
}

/// One universal `common` param, as a function of the manifest it is added to. It may read the
/// manifest's static shape, but never `m.params` for a `common` key — that is a half-built world.
type CommonDecl = fn(&NodeManifest) -> ParamDecl;

/// Run on the node's own schedule instead of waiting for an input frame; defaults to `m.producer`.
fn autotrigger(m: &NodeManifest) -> ParamDecl {
    ParamDecl {
        group: "common",
        name: "autotrigger",
        spec: ParamSpec::Bool { default: m.producer },
        expression: None,
        doc: Some(
            "Run on the node's own schedule, instead of waiting for an input frame. \
             Turn this on for sources; leave it off for transforms driven by their input.",
        ),
        section: 0,
        show: None,
    }
}

/// The rate cap, carried by every node as a `variables.system.default_ufreq` expression and live on a
/// producer. `trigger: true` is inert here — a `common.*` arrival never triggers a run.
fn max_frequency(m: &NodeManifest) -> ParamDecl {
    ParamDecl {
        group: "common",
        name: "max_frequency",
        spec: ParamSpec::Float { default: 0.0, min: 0.0, max: 100.0 },
        expression: Some(ExprDecl {
            source: "variables.system.default_ufreq",
            mode: if m.producer { ExprMode::On } else { ExprMode::Off },
            trigger: true,
        }),
        doc: Some(
            "Rate cap for this node, read through `frequency_mode`. 0 means uncapped — the node \
             runs as often as the scheduler and its inputs allow.",
        ),
        section: 0,
        show: None,
    }
}

/// How to read [`max_frequency`]: a rate, or a period.
fn frequency_mode(_: &NodeManifest) -> ParamDecl {
    ParamDecl {
        group: "common",
        name: "frequency_mode",
        spec: ParamSpec::Str {
            default: FREQ_MODE_UPDATES_PER_SECOND,
            options: &[FREQ_MODE_UPDATES_PER_SECOND, FREQ_MODE_SECONDS_PER_UPDATE],
            refresh: false,
        },
        expression: None,
        doc: Some(
            "How to read `max_frequency`: as a rate in Hz (updates per second), or as a period \
             in seconds between updates — convenient for very slow nodes.",
        ),
        section: 0,
        show: None,
    }
}

/// The universal `common` scheduling group; a fourth param is added here and nowhere else.
static COMMON_DECLS: &[CommonDecl] = &[autotrigger, max_frequency, frequency_mode];

pub fn common_decls(m: &NodeManifest) -> impl Iterator<Item = ParamDecl> + '_ {
    COMMON_DECLS.iter().map(move |d| d(m))
}
