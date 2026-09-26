//! The audio node's author contract: the `AudioNode` trait, the block it is handed, and the
//! conventions every signal in the engine follows — shared by the engine that runs a node and
//! the file that authors one, so the two halves cannot drift.

pub use goofi_core;
pub use goofi_node::{ExprDecl, ExprMode, OutputDecl, ParamDecl, ParamSpec, Show, SlotDecl, Tag};

pub mod abi;
#[cfg(feature = "host")]
pub mod host;

/// Every block is exactly this many frames; the engine carries any surplus to the next callback.
pub const BLOCK: usize = 64;
/// What a node is prepared for; a port never carries more channels than this.
pub const MAX_CHANNELS: u16 = 16;
/// The most inputs, outputs or AUDIO-RATE params a node may declare: a block's ports are stack
/// arrays on both sides of the boundary, so this is stack per callback rather than a free number.
/// A control-rate param is a float in `Block::scalars` instead, and is not bounded by this.
pub const MAX_PORTS: usize = 64;

/// What a node file declares: a `NodeManifest` less the type name, which is the FILE's. The
/// signal-only slot flags are ignored; `multi: true` on an input sums its wires at the jack.
pub struct Manifest {
    pub tags: &'static [Tag],
    /// What the type IS. The FIRST LINE is the nutshell a catalog shows and all most readers
    /// see; whatever follows it is the detail `library get` answers.
    pub doc: &'static str,
    pub inputs: &'static [SlotDecl],
    pub outputs: &'static [OutputDecl],
    pub params: &'static [ParamDecl],
}

/// The params a node declares, as ONE list that is both the manifest's slice and the indices a
/// node reads them by: `params! { CUTOFF = ParamDecl { …, section: 0, show: None }, GAIN = ParamDecl { …, section: 0, show: None } }` yields
/// `PARAMS` and `P::CUTOFF == 0`, `P::GAIN == 1`.
#[macro_export]
macro_rules! params {
    ($($name:ident = $decl:expr),* $(,)?) => {
        pub static PARAMS: &[$crate::ParamDecl] = &[$($decl),*];
        pub mod P {
            $crate::params!(@index 0usize; $($name)*);
        }
    };
    (@index $i:expr; $name:ident $($rest:ident)*) => {
        pub const $name: usize = $i;
        $crate::params!(@index $i + 1usize; $($rest)*);
    };
    (@index $i:expr;) => {};
}

/// One input or param for one block: planar channels of `BLOCK` frames each. A param's region
/// holds its one source, settled for this block.
pub struct Port<'a> {
    channels: u16,
    wired: bool,
    data: &'a [f32],
}

impl<'a> Port<'a> {
    /// `data` holds `channels` planar blocks; `wired` is false for the shared silence an unwired
    /// input reads.
    pub fn new(data: &'a [f32], channels: u16, wired: bool) -> Port<'a> {
        debug_assert_eq!(data.len(), channels as usize * BLOCK);
        Port { channels, wired, data }
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// `false` when nothing is wired: the data is silence and `channels` is 1.
    pub fn wired(&self) -> bool {
        self.wired
    }

    /// Channel `c` of a port that may be narrower than the block it feeds: a one-channel port is
    /// on every channel, a channel past a wider port's count is silence.
    pub fn chan(&self, c: usize) -> &[f32; BLOCK] {
        let c = if self.channels == 1 { 0 } else { c };
        if c >= self.channels as usize {
            return &SILENT;
        }
        self.data[c * BLOCK..(c + 1) * BLOCK].try_into().expect("a channel is BLOCK frames")
    }
}

static SILENT: [f32; BLOCK] = [0.0; BLOCK];

/// One output for one block, with the channel count `channels()` answered for this plan.
pub struct PortMut<'a> {
    channels: u16,
    data: &'a mut [f32],
}

impl<'a> PortMut<'a> {
    pub fn new(data: &'a mut [f32], channels: u16) -> PortMut<'a> {
        debug_assert_eq!(data.len(), channels as usize * BLOCK);
        PortMut { channels, data }
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn chan_mut(&mut self, c: usize) -> &mut [f32; BLOCK] {
        (&mut self.data[c * BLOCK..(c + 1) * BLOCK]).try_into().expect("a channel is BLOCK frames")
    }
}

/// One block: every declared input and output, in declaration order — wires already summed and
/// coerced. `params` carries a port for each AUDIO-RATE param; `scalars` carries this block's
/// value for EVERY declared param, so a control-rate one costs no port and no ceiling.
pub struct Block<'a> {
    pub ins: &'a [Port<'a>],
    pub outs: &'a mut [PortMut<'a>],
    pub params: &'a [Port<'a>],
    pub scalars: &'a [f32],
}

/// The DSP half of an audio node. It moves to the audio thread inside the plan and owns
/// arithmetic only: no allocation, no lock, no syscall in `process`.
pub trait AudioNode: Send {
    /// Per-output channel counts for these per-input counts — ports first, then referenced
    /// params, in declaration order — and the settled scalar params. Pure; evaluated at plan
    /// compile, on the control thread. The default is `max(ins).max(1)` for each of the `outs`.
    fn channels(&self, ins: &[u16], _params: &[f64], outs: usize) -> Vec<u16> {
        vec![ins.iter().copied().max().unwrap_or(1).max(1); outs]
    }
    /// How many LEADING params this node reads per sample, out of the `declared` it has. The rest
    /// are control rate: they reach `process` through `Block::scalars`, cost no port, and so are
    /// not bounded by `MAX_PORTS`. The default keeps every param audio rate.
    fn audio_params(&self, declared: usize) -> usize {
        declared
    }
    /// Once on the control thread before the first block, again only when the rate changes.
    /// Allocate here, for `MAX_CHANNELS` and `BLOCK` frames.
    fn prepare(&mut self, rate: f64);
    /// One block, on the audio thread.
    fn process(&mut self, b: &mut Block<'_>);
    /// `true` for a type whose outputs come from the PREVIOUS block's inputs — the one way a
    /// loop closes. The sort ignores its in-edges; it runs first each block on last block's
    /// regions.
    fn feedback(&self) -> bool {
        false
    }
    /// State beyond the params, as opaque bytes. A param value is never in it, and a node that
    /// returns nothing leaves nothing behind.
    fn save(&self) -> Vec<u8> {
        Vec::new()
    }
    fn load(&mut self, _bytes: &[u8]) {}
}

/// The conventions, stated once: a bipolar signal lives in `[-1, 1]` and `1` is full scale; a
/// unipolar one in `[0, 1]`; a gate is HIGH above zero; pitch is volts per octave, zero at C4,
/// so transposition is an addition.
pub const C4_HZ: f32 = 261.63;

/// Whether a gate is HIGH, by goofi's one rule for it — the same one the signal plane reads.
pub fn high(v: f32) -> bool {
    goofi_node::mailbox::gate(v as f64)
}

pub fn hz_of(pitch: f32) -> f32 {
    C4_HZ * 2f32.powf(pitch)
}

/// The centre of band `b` of `bands`, in volts, spread evenly from `low` to `high` — the one
/// layout a bank that measures and a bank that applies both read, so their channels line up.
pub fn band_volts(b: usize, bands: usize, low: f32, high: f32) -> f32 {
    match bands > 1 {
        true => low + (high - low) * b as f32 / (bands - 1) as f32,
        false => low,
    }
}

/// Band `b` of a bank standing on `voices` pitches: which voice it takes, and how far above that
/// voice's own pitch it sits, in volts. Bands take the voices in turn, so each voice gets every
/// `voices`th band as its next partial.
pub fn band_partial(b: usize, voices: usize) -> (usize, f32) {
    let voices = voices.max(1);
    (b % voices, ((b / voices + 1) as f32).log2())
}


/// What the crossings into the audio plane share: the range a picture's numbers are named by,
/// and how a signal frame is played — one vocabulary, so two crossings cannot mean different
/// things by one word.
pub mod cross {
    /// The `mode` options, in the order [`ranged`] reads their index.
    pub const RANGES: &[&str] = &["direct", "bipolar", "unipolar"];

    /// The options of a `mode` that sets how the engine PLAYS the frames a node takes in: each
    /// looped until the next, each value a sine at that many Hz, or each looped with its rows
    /// mixed to one channel. A float `smoothing` beside it, in seconds, crossfades one frame into
    /// the next, or glides each sine to its new pitch.
    pub const PLAYBACK: &[&str] = &["waveform", "oscillator", "mix"];

    /// `v` on the plane it is crossing into: itself, or its place in `lo..hi` as a full-scale
    /// bipolar or unipolar signal. `min`..`max` is a declaration, so what falls outside it is
    /// held at the end it left by.
    pub fn ranged(mode: u8, v: f32, lo: f32, hi: f32) -> f32 {
        if mode == 0 {
            return v;
        }
        // A range of no width has no place in it; the low end is the whole of it.
        let t = if hi > lo { ((v - lo) / (hi - lo)).clamp(0.0, 1.0) } else { 0.0 };
        match mode {
            1 => t * 2.0 - 1.0,
            _ => t,
        }
    }
}

/// A rising-edge detector over a gate: `true` on the sample the gate goes HIGH.
#[derive(Default)]
pub struct Edge {
    high: bool,
}

impl Edge {
    pub fn rising(&mut self, v: f32) -> bool {
        let high = high(v);
        let rose = high && !self.high;
        self.high = high;
        rose
    }
}
