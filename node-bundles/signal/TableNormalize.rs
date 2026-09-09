//! Scale each table member from its own history.

use goofi_core::normalize::{apply, scale_of, validate, Running};
use goofi_core::{indexmap::IndexMap, Data, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, ParamKey, Params, ParamSpec, SlotDecl, Tag};

#[derive(Default)]
struct Member {
    running: Running,
    past: Vec<f32>,
}

#[derive(Default)]
struct TableNormalize {
    /// One set of statistics per member NAME, so a table that gains a key does not disturb the rest.
    seen: IndexMap<String, Member>,
}

impl Node for TableNormalize {
    fn process(&mut self, inp: &Inputs<'_>, out: &mut Outputs<'_>, _c: &mut NodeCtx, p: &Params<'_>) -> NodeResult {
        let d = inp.get("input").ok_or("`input` is required")?;
        let table = d.as_table()?;
        let mode = p.str("normalize", "mode").unwrap_or("zscore");
        let size = p.f64("window", "size").unwrap_or(0.0);
        let unit = p.str("window", "unit").unwrap_or("samples");
        let hold = p.bool("window", "hold").unwrap_or(false);

        let mut scaled: IndexMap<String, Data> = IndexMap::with_capacity(table.len());
        for (name, member) in table {
            let Ok(a) = member.as_array() else {
                scaled.insert(name.clone(), member.clone());
                continue;
            };
            let meta = if unit == "seconds (ufreq)" { d.meta() } else { member.meta() };
            let window = goofi_core::stream::window_count(size, unit, meta)?;
            validate(mode, window)?;
            let values: Vec<f32> =
                a.as_bytes().chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().expect("four bytes"))).collect();
            let run = self.seen.entry(name.clone()).or_default();
            if !hold {
                for x in &values {
                    run.running.push(*x as f64);
                }
                if window > 0 {
                    run.past.extend_from_slice(&values);
                    run.past.drain(..run.past.len().saturating_sub(window));
                }
            }
            let scale = if window > 0 {
                scale_of(mode, &run.past)
            } else {
                run.running.scale(mode)?
            };
            let buf: Vec<u8> = values.iter().flat_map(|x| apply(scale, *x).to_le_bytes()).collect();
            let frame = Data::array_f32(a.shape().to_vec(), buf, member.meta().clone()).map_err(|e| e.to_string())?;
            scaled.insert(name.clone(), frame);
        }
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
        group: "window",
        name: "unit",
        spec: ParamSpec::Str { default: "samples", options: &["samples", "seconds", "seconds (ufreq)"], refresh: false },
        expression: None,
        doc: Some("What size counts for each member. Seconds uses the member's sfreq or ufreq; seconds (ufreq) uses the table's update rate."),
    },
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
             instead. Robust mode needs a positive size.",
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
