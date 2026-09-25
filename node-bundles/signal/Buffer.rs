//! Buffer — a rolling window along one axis, one window per position along the other axes.
//! An axis past the rank is a new trailing one, and each frame is one entry along it.

use goofi_core::{resolve_axis, Axis, Data, SlotType};
use goofi_signal_sdk::{Inputs, Manifest, Node, NodeCtx, NodeResult, OutputDecl, Outputs, ParamDecl, Params, ParamSpec, SlotDecl, Tag};

#[derive(Default)]
struct Buffer {
    /// One window per position along the axes OUTSIDE the buffered one, as raw f32-LE bytes.
    windows: Vec<Vec<u8>>,
    /// The shape the windows were filled from, with the buffered axis set to 0.
    layout: Vec<usize>,
}

impl Node for Buffer {
    fn process(
        &mut self,
        inp: &Inputs<'_>,
        out: &mut Outputs<'_>,
        _c: &mut NodeCtx,
        p: &Params<'_>,
    ) -> NodeResult {
        let d = inp.get("input").ok_or("`input` is required")?;
        let unit = p.str("buffer", "unit").unwrap_or("seconds");
        let size = goofi_core::stream::window_count(p.f64("buffer", "size").unwrap_or(2.0), unit, d.meta())?.max(1);
        let a = d.assert_ndims().at_least(1)?;
        let along = if matches!(unit, "updates" | "seconds (ufreq)") {
            a.shape().len() as i64
        } else {
            p.i64("buffer", "axis").unwrap_or(-1)
        };
        if along >= 255 { return Err("buffer axis must be less than 255".into()); }
        let mut shape = a.shape().to_vec();
        if along >= 0 {
            shape.resize(shape.len().max(along as usize + 1), 1);
        }
        let axis = resolve_axis(along, shape.len())?;

        let stride: usize = shape[axis + 1..].iter().product::<usize>() * 4;
        let outer: usize = shape[..axis].iter().product();
        let block = shape[axis] * stride;

        let mut layout = shape.to_vec();
        layout[axis] = 0;
        if layout != self.layout {
            self.windows = vec![Vec::new(); outer];
            self.layout = layout;
        }

        if stride == 0 { return Err("buffer needs nonempty dimensions outside its axis".into()); }
        let cap = size.checked_mul(stride).filter(|n| *n <= isize::MAX as usize)
            .ok_or("buffer size exceeds the supported allocation")?;
        let src = a.as_bytes();
        for (o, window) in self.windows.iter_mut().enumerate() {
            window.extend_from_slice(&src[o * block..(o + 1) * block]);
            if window.len() > cap {
                window.drain(..window.len() - cap);
            }
        }

        let kept = self.windows.first().map_or(0, |w| w.len() / stride);
        let mut buf = Vec::with_capacity(outer * kept * stride);
        for window in &self.windows {
            buf.extend_from_slice(window);
        }

        let mut shape_out = shape.clone();
        shape_out[axis] = kept;
        // Only the buffered axis' labels are dropped: they name entries that have rolled past.
        let mut meta = d.meta().clone();
        if meta.channels().get(axis).is_some_and(|x| !x.is_empty()) {
            let axes = meta.channels().clone().with(axis, Axis::default());
            meta.set_channels(axes);
        }
        out.set("out", Data::array_f32(shape_out, buf, meta).map_err(|e| e.to_string())?);
        Ok(())
    }
}

static PARAMS: &[ParamDecl] = &[
    ParamDecl {
        group: "buffer",
        name: "unit",
        spec: ParamSpec::Str { default: "seconds", options: &["seconds", "samples", "seconds (ufreq)", "updates"], refresh: false },
        expression: None,
        doc: Some("Seconds uses sfreq, or ufreq if no sample rate is set, along the selected axis. Updates and seconds (ufreq) stack whole frames on a new trailing axis."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "buffer",
        name: "size",
        spec: ParamSpec::Float { default: 2.0, min: 0.001, max: 60.0 },
        expression: None,
        doc: Some("How much recent data to keep, in the selected unit."),
        section: 0,
        show: None,
    },
    ParamDecl {
        group: "buffer",
        name: "axis",
        spec: ParamSpec::Int { default: -1, min: -8, max: 7, options: &[-2, -1, 0, 1, 2] },
        expression: None,
        doc: Some(
            "Which axis to roll along, negative from the end. -1 is time, the default and the \
             one a signal is usually buffered over; -2 is channels. An axis past the rank is a \
             new one, so a 1-D frame buffered along axis 1 becomes channels by time.",
        ),
        section: 0,
        show: None,
    },
];
static INPUTS: &[SlotDecl] = &[SlotDecl {
    name: "input",
    kind: SlotType::Array,
    trigger_process: true,
    multi: false,
    required: true,
}];
static OUTPUTS: &[OutputDecl] = &[OutputDecl {
    name: "out",
    kind: SlotType::Array,
}];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Transform],
    doc: "A rolling window along one axis.\n\
          Keeps the most recent `size` entries; an axis past the rank is a new one.",
    inputs: INPUTS,
    outputs: OUTPUTS,
    params: PARAMS,
    producer: false,
};

goofi_signal_sdk::export!(Buffer, MANIFEST);
