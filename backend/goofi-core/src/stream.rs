//! Bounded input history. Each frame is appended in full.

/// One input's recent past, held time-major so a step is `stride` contiguous bytes.
#[derive(Default)]
pub struct Stream {
    stride: usize,
    data: Vec<u8>,
}

/// The dimensions outside `dim`, `dim` itself, and the dimensions inside it.
fn split(shape: &[usize], dim: usize) -> (usize, usize, usize) {
    (
        shape[..dim].iter().product(),
        shape.get(dim).copied().unwrap_or(0),
        shape[dim + 1..].iter().product(),
    )
}

fn reorder(shape: &[usize], dim: usize, src: &[u8], to_time_major: bool) -> Vec<u8> {
    let (outer, steps, inner) = split(shape, dim);
    let ib = inner * 4;
    if outer <= 1 {
        return src.to_vec();
    }
    let mut out = vec![0u8; src.len()];
    for t in 0..steps {
        for o in 0..outer {
            let (a, b) = (((o * steps) + t) * ib, ((t * outer) + o) * ib);
            let (from, to) = if to_time_major { (a, b) } else { (b, a) };
            out[to..to + ib].copy_from_slice(&src[from..from + ib]);
        }
    }
    out
}

/// The lanes of `src` along `dim`: one run of values per position on the other axes, which is
/// the shape every stitching node works in.
pub fn lanes(shape: &[usize], dim: usize, src: &[u8]) -> Vec<Vec<f32>> {
    let (outer, steps, inner) = split(shape, dim);
    let mut out = Vec::with_capacity(outer * inner);
    for o in 0..outer {
        for i in 0..inner {
            let at = |k: usize| (((o * steps) + k) * inner + i) * 4;
            out.push(
                (0..steps)
                    .map(|k| f32::from_le_bytes(src[at(k)..at(k) + 4].try_into().expect("four bytes")))
                    .collect(),
            );
        }
    }
    out
}

/// Lay lanes back into `shape` along `dim` — the inverse of [`lanes`].
pub fn unlanes(shape: &[usize], dim: usize, lanes: &[Vec<f32>]) -> Vec<u8> {
    let (outer, steps, inner) = split(shape, dim);
    let mut out = vec![0u8; outer * steps * inner * 4];
    for o in 0..outer {
        for i in 0..inner {
            let lane = &lanes[o * inner + i];
            for (k, v) in lane.iter().enumerate().take(steps) {
                let at = (((o * steps) + k) * inner + i) * 4;
                out[at..at + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
    }
    out
}

impl Stream {
    pub fn new() -> Stream {
        Stream::default()
    }

    /// Forget the past — what a `reset` pulse clears.
    pub fn reset(&mut self) {
        self.data.clear();
        self.stride = 0;
    }

    /// The steps held right now.
    pub fn steps(&self) -> usize {
        self.data.len().checked_div(self.stride).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.steps() == 0
    }

    /// Append a full frame after at most `history` steps. Return the combined frame and its offset.
    pub fn push(
        &mut self,
        shape: &[usize],
        dim: usize,
        frame: &[u8],
        history: usize,
    ) -> (Vec<usize>, Vec<u8>, usize) {
        let (_, steps, _) = split(shape, dim);
        let stride = frame.len().checked_div(steps).unwrap_or(0);
        if stride == 0 {
            return (shape.to_vec(), frame.to_vec(), 0);
        }
        if stride != self.stride {
            self.data.clear();
            self.stride = stride;
        }
        let keep = history.min(self.steps());
        let cut = self.data.len() - keep * stride;
        self.data.drain(..cut);

        let tm = reorder(shape, dim, frame, true);
        let offset = self.steps();
        self.data.extend_from_slice(&tm);

        let mut out_shape = shape.to_vec();
        out_shape[dim] = self.steps();
        (out_shape.clone(), reorder(&out_shape, dim, &self.data, false), offset)
    }
}

/// Convert a window size to a count. Seconds use the selected metadata rate.
pub fn window_count(size: f64, unit: &str, meta: &crate::Meta) -> Result<usize, String> {
    if !size.is_finite() || size < 0.0 {
        return Err("size must be finite and nonnegative".into());
    }
    if size == 0.0 { return Ok(0); }
    let rate = match unit {
        "samples" | "updates" => 1.0,
        "seconds" => meta.sfreq().or_else(|| meta.ufreq()).ok_or("seconds requires sfreq or ufreq metadata")?,
        "seconds (ufreq)" => meta.ufreq().ok_or("seconds (ufreq) requires ufreq metadata")?,
        _ => return Err(format!("unknown size unit: {unit}")),
    };
    if !rate.is_finite() || rate <= 0.0 {
        return Err("the selected rate must be finite and positive".into());
    }
    let count = (size * rate).round();
    if !count.is_finite() || count >= isize::MAX as f64 / 4.0 {
        return Err("size exceeds the supported window length".into());
    }
    Ok(count as usize)
}
