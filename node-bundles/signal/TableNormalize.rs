//! TableNormalize — put every member of a table on a common scale, each against its OWN past.
//!
//! `Normalize` scales along an axis INSIDE one frame, which is what a signal wants. A table is a
//! bag of named features whose units have nothing to do with each other, so the only axis that
//! means anything is time: each member is measured against the history of that member.

use goofi_core::{indexmap::IndexMap, Data, Meta, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamKey, Params, ParamSpec, SlotDecl, Tag};

/// What a member is scaled by: a centre and a spread, whatever the mode calls them.
#[derive(Clone, Copy, Default)]
struct Scale {
    centre: f64,
    spread: f64,
}

/// Running count, mean and sum of squared deviations, and the extremes — one per member.
#[derive(Clone, Default)]
struct Running {
    n: f64,
    mean: f64,
    m2: f64,
    low: f64,
    high: f64,
    /// The window's own past, kept only when a window was asked for.
    past: Vec<f32>,
}

impl Running {
    fn push(&mut self, x: f64, window: usize) {
        if self.n == 0.0 {
            (self.low, self.high) = (x, x);
        }
        self.n += 1.0;
        let delta = x - self.mean;
        self.mean += delta / self.n;
        self.m2 += delta * (x - self.mean);
        self.low = self.low.min(x);
        self.high = self.high.max(x);
        if window > 0 {
            self.past.push(x as f32);
            let over = self.past.len().saturating_sub(window);
            self.past.drain(..over);
        }
    }

    fn scale(&self, mode: &str, window: usize) -> Scale {
        if window > 0 {
            return scale_of(mode, &self.past);
        }
        match mode {
            "minmax" => Scale { centre: self.low, spread: self.high - self.low },
            _ => Scale { centre: self.mean, spread: (self.m2 / self.n.max(1.0)).sqrt() },
        }
    }
}

fn median(sorted: &[f64], at: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let pos = at * (sorted.len() - 1) as f64;
    let (low, frac) = (pos.floor() as usize, pos.fract());
    sorted[low] + (sorted[(low + 1).min(sorted.len() - 1)] - sorted[low]) * frac
}

/// The statistics of one window, by mode — the same three `Normalize` offers.
fn scale_of(mode: &str, window: &[f32]) -> Scale {
    match mode {
        "minmax" => {
            let low = window.iter().copied().fold(f32::INFINITY, f32::min) as f64;
            let high = window.iter().copied().fold(f32::NEG_INFINITY, f32::max) as f64;
            Scale { centre: low, spread: high - low }
        }
        "robust" => {
            let mut v: Vec<f64> = window.iter().map(|x| *x as f64).collect();
            v.sort_by(f64::total_cmp);
            Scale { centre: median(&v, 0.5), spread: median(&v, 0.75) - median(&v, 0.25) }
        }
        _ => {
            let n = window.len().max(1) as f64;
            let mean = window.iter().map(|x| *x as f64).sum::<f64>() / n;
            let var = window.iter().map(|x| (*x as f64 - mean).powi(2)).sum::<f64>() / n;
            Scale { centre: mean, spread: var.sqrt() }
        }
    }
}

/// A spread of zero means the member has not varied, and every value sits at the centre.
fn apply(scale: Scale, x: f32) -> f32 {
    if scale.spread == 0.0 {
        0.0
    } else {
        ((x as f64 - scale.centre) / scale.spread) as f32
    }
}

#[derive(Default)]
struct TableNormalize {
    /// One set of statistics per member NAME, so a table that gains a key does not disturb the rest.
    seen: IndexMap<String, Running>,
    held: bool,
}

impl Node for TableNormalize {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, _c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let d = inp.get("input").ok_or("`input` is required")?;
        let table = d.as_table()?;
        let mode = p.str("normalize", "mode").unwrap_or("zscore");
        let window = p.f64("window", "size").unwrap_or(0.0).max(0.0).round() as usize;
        let hold = p.bool("window", "hold").unwrap_or(false);

        let mut scaled: IndexMap<String, Data> = IndexMap::with_capacity(table.len());
        for (name, member) in table {
            let Ok(a) = member.as_array() else {
                // A member that is not an array — a string, a nested table — passes through whole.
                scaled.insert(name.clone(), member.clone());
                continue;
            };
            let values: Vec<f32> =
                a.as_bytes().chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().expect("four bytes"))).collect();
            let run = self.seen.entry(name.clone()).or_default();
            if !hold {
                // A member is one observation per frame, however wide it is: an array's own values
                // are the same quantity at different places, so they share the member's statistics.
                for x in &values {
                    run.push(*x as f64, window);
                }
            }
            let scale = run.scale(mode, window);
            let buf: Vec<u8> = values.iter().flat_map(|x| apply(scale, *x).to_le_bytes()).collect();
            let frame = Data::array_f32(a.shape().to_vec(), buf, member.meta().clone()).map_err(|e| e.to_string())?;
            scaled.insert(name.clone(), frame);
        }
        self.held = hold;
        out.set("out", Data::table(scaled, d.meta().clone()));
        Ok(())
    }

    fn on_pulse(&mut self, _key: &ParamKey, _p: &Params<'_>) -> NodeResult {
        self.seen.clear();
        Ok(())
    }
}

static INPUTS: &[SlotDecl] =
    &[SlotDecl { name: "input", kind: SlotType::Table, trigger_process: true, multi: false, required: true }];

static OUTPUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Table }];

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "normalize",
        name: "mode",
        spec: ParamSpec::Str { default: "zscore", options: &["zscore", "minmax", "robust"], refresh: false },
        expression: None,
        doc: Some(
            "`zscore` measures in standard deviations from the mean, `minmax` maps the past onto \
             0 to 1, and `robust` uses the median and the middle half, which an outlier cannot move.",
        ),
    },
    ParamDecl {
        group: "window",
        name: "size",
        spec: ParamSpec::Float { default: 0.0, min: 0.0, max: 100_000.0 },
        expression: None,
        doc: Some(
            "How many past values each member is measured against. 0 keeps running statistics \
             instead, which costs no memory as it grows but never forgets.",
        ),
    },
    ParamDecl {
        group: "window",
        name: "hold",
        spec: ParamSpec::Bool { default: false },
        expression: None,
        doc: Some("Stop taking in new values and keep scaling by what is already known."),
    },
    ParamDecl {
        group: "window",
        name: "reset",
        spec: ParamSpec::Pulse,
        expression: None,
        doc: Some("Forget every member's statistics and start again."),
    },
];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Transform],
    doc: "Put every member of a table on a common scale.\n\
          Each member is measured against its OWN past, so features in different units become \
          comparable numbers without one of them dominating.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(TableNormalize, MANIFEST);
