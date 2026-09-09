#[derive(Clone, Copy, Default)]
pub struct Scale {
    centre: f64,
    spread: f64,
}

#[derive(Clone, Copy, Default)]
pub struct Running {
    n: f64,
    mean: f64,
    m2: f64,
    low: f64,
    high: f64,
}

impl Running {
    pub fn push(&mut self, x: f64) {
        if self.n == 0.0 {
            (self.low, self.high) = (x, x);
        }
        self.n += 1.0;
        let delta = x - self.mean;
        self.mean += delta / self.n;
        self.m2 += delta * (x - self.mean);
        self.low = self.low.min(x);
        self.high = self.high.max(x);
    }

    pub fn scale(&self, mode: &str) -> Result<Scale, String> {
        validate(mode, 0)?;
        Ok(match mode {
            "minmax" => Scale { centre: self.low, spread: self.high - self.low },
            _ => Scale { centre: self.mean, spread: (self.m2 / self.n.max(1.0)).sqrt() },
        })
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

pub fn scale_of(mode: &str, window: &[f32]) -> Scale {
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

pub fn apply(scale: Scale, x: f32) -> f32 {
    if scale.spread == 0.0 {
        0.0
    } else {
        ((x as f64 - scale.centre) / scale.spread) as f32
    }
}

pub fn validate(mode: &str, window: usize) -> Result<(), String> {
    if mode == "robust" && window == 0 {
        return Err("robust normalization needs a positive window size".into());
    }
    Ok(())
}
