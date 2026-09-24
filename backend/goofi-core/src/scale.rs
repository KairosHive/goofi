//! Musical scales made from a few numbers, so any scale is a setting rather than a table. A scale
//! is stacked from a generator (the pentatonic and diatonic families and every moment-of-symmetry
//! scale), read off the harmonic series, or chosen as a subset of an equal division; `mode` then
//! rotates it onto another of its degrees. Degrees are cents above the root, inside one period.
//! A node that quantizes to a scale declares these numbers in its `scale` param group.

/// The most degrees a scale has: 53 parts to the octave is the finest division theory names.
pub const MAX_DEGREES: usize = 53;

/// The parts of a division a mask can name: an audio param is an f32, exact to 24 bits.
pub const MASK_BITS: u32 = 24;

/// C4, the pitch at 0 V on the audio plane and the C a `root` counts from on the signal plane.
pub const C4_HZ: f64 = 261.625_565_300_598_6;

/// How a custom scale is made, in the order a `method` param offers them.
pub const METHODS: &[&str] = &["generator", "harmonics", "division"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Method {
    /// `steps` stacks of `generator` cents, folded into the period.
    Generator,
    /// Partials `steps` to `2 * steps - 1` of a harmonic series, folded into the period.
    Harmonics,
    /// The period split into `steps` equal parts, of which `mask` admits some (0 admits all); a
    /// part past the mask's bits is admitted only when the mask is 0.
    Division,
}

/// A scale as numbers: every field is a param, so a preset is only a set of values for them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Recipe {
    pub method: Method,
    /// The interval the scale repeats at, in cents; 1200 is the octave.
    pub period: f64,
    pub generator: f64,
    pub steps: u32,
    pub mask: u64,
    /// The degree the scale is read from, so one recipe answers all of its modes.
    pub mode: i32,
}

const fn fifths(steps: u32, mode: i32) -> Recipe {
    Recipe { method: Method::Generator, period: 1200.0, generator: 700.0, steps, mask: 0, mode }
}

/// A subset of twelve-tone equal temperament, by semitone.
const fn twelve(semitones: &[u32]) -> Recipe {
    let (mut mask, mut i) = (0, 0);
    while i < semitones.len() {
        mask |= 1 << semitones[i];
        i += 1;
    }
    Recipe { method: Method::Division, period: 1200.0, generator: 700.0, steps: 12, mask, mode: 0 }
}

/// The named scales. Seven fifths read from their fourth degree are the major scale, and each
/// church mode is the same stack read from another one.
const PRESETS: [(&str, Recipe); 14] = [
    ("chromatic", twelve(&[])),
    ("major", fifths(7, 4)),
    ("minor", fifths(7, 2)),
    ("dorian", fifths(7, 5)),
    ("phrygian", fifths(7, 6)),
    ("lydian", fifths(7, 0)),
    ("mixolydian", fifths(7, 1)),
    ("locrian", fifths(7, 3)),
    ("pentatonic_major", fifths(5, 0)),
    ("pentatonic_minor", fifths(5, 4)),
    ("blues", twelve(&[0, 3, 5, 6, 7, 10])),
    ("harmonic_minor", twelve(&[0, 2, 3, 5, 7, 8, 11])),
    ("whole_tone", Recipe { method: Method::Generator, period: 1200.0, generator: 200.0, steps: 6, mask: 0, mode: 0 }),
    ("harmonic", Recipe { method: Method::Harmonics, period: 1200.0, generator: 700.0, steps: 8, mask: 0, mode: 0 }),
];

/// The options of a `scale` param: `custom`, which reads the recipe off the other params, then
/// every preset.
pub const SCALES: [&str; PRESETS.len() + 1] = {
    let mut names = ["custom"; PRESETS.len() + 1];
    let mut i = 0;
    while i < PRESETS.len() {
        names[i + 1] = PRESETS[i].0;
        i += 1;
    }
    names
};

impl Recipe {
    /// The recipe the `scale` param's option `index` names, or `custom` when it names none.
    pub fn chosen(index: usize, custom: Recipe) -> Recipe {
        index.checked_sub(1).and_then(|i| PRESETS.get(i)).map_or(custom, |(_, r)| *r)
    }

    /// A custom recipe from its params' scalars, each held to what it can mean.
    pub fn from_scalars(method: f64, period: f64, generator: f64, steps: f64, mask: f64, mode: f64) -> Recipe {
        let method = match method as usize {
            0 => Method::Generator,
            1 => Method::Harmonics,
            _ => Method::Division,
        };
        let period = if period.is_finite() && period > 0.0 { period } else { 1200.0 };
        let steps = (steps.max(1.0) as u32).min(MAX_DEGREES as u32);
        Recipe { method, period, generator, steps, mask: mask.max(0.0) as u64, mode: mode as i32 }
    }

    /// The degrees, ascending, in `out`; answers how many. Never allocates, so the audio thread
    /// builds one per block.
    pub fn degrees(&self, out: &mut [f64; MAX_DEGREES]) -> usize {
        let (p, steps) = (self.period, self.steps.clamp(1, MAX_DEGREES as u32) as usize);
        let mut n = 0;
        for k in 0..steps {
            let cents = match self.method {
                Method::Generator => k as f64 * self.generator,
                Method::Harmonics => 1200.0 * ((steps + k) as f64 / steps as f64).log2(),
                Method::Division if self.mask == 0 || (k < MASK_BITS as usize && self.mask >> k & 1 == 1) => k as f64 * p / steps as f64,
                Method::Division => continue,
            };
            out[n] = cents.rem_euclid(p);
            n += 1;
        }
        let d = &mut out[..n];
        d.sort_by(f64::total_cmp);
        let mut kept = 0;
        for i in 0..n {
            if kept == 0 || d[i] - d[kept - 1] > 1e-6 {
                d[kept] = d[i];
                kept += 1;
            }
        }
        if kept == 0 {
            return 0;
        }
        // The mode: the same intervals, counted from another degree.
        let from = d[self.mode.rem_euclid(kept as i32) as usize];
        for x in &mut d[..kept] {
            *x = (*x - from).rem_euclid(p);
        }
        d[..kept].sort_by(f64::total_cmp);
        kept
    }

    /// `cents` above the root pulled onto the nearest degree, in the period it came from, and
    /// which degree that is. The first degree of the next period up is a candidate too, so a pitch
    /// just under it climbs rather than falling a whole step.
    pub fn snap(&self, degrees: &[f64], cents: f64) -> (f64, usize) {
        let n = degrees.len();
        if n == 0 {
            return (cents, 0);
        }
        let p = self.period;
        let register = (cents / p).floor();
        let inside = cents - register * p;
        let i = degrees.partition_point(|d| *d < inside);
        let below = if i == 0 { (degrees[n - 1] - p, n - 1) } else { (degrees[i - 1], i - 1) };
        let above = if i == n { (degrees[0] + p, 0) } else { (degrees[i], i) };
        let (nearest, degree) = if inside - below.0 <= above.0 - inside { below } else { above };
        (register * p + nearest, degree)
    }
}
