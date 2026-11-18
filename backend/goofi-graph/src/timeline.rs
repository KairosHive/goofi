//! The timeline: tracks of notes and of keyframed curves on the patch clock's position. A track
//! is a group of variables: a curve track is ONE functional of `t` over the whole track, and a
//! note track is the velocities held at the position.

use std::collections::HashMap;
use std::sync::Arc;

use goofi_core::ease::Curve;
use goofi_core::scale;
use goofi_core::variables::MIDI_ENTRIES;
use goofi_core::{Data, Face, Meta};
use goofi_node::ExprEvaluator;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::expr_rewrite::Target;
use crate::machine::{functional_code, AttributeKind, Keyed, Leg, Motion};

/// The one element of a note track's group, and its width: each held note's velocity, where a
/// MIDI device's `notes` has its channel 1, so a reader of a device reads a track.
pub(crate) const NOTES: (&str, usize) = MIDI_ENTRIES[1];
/// The grids a note track's notes snap to in a panel, by name: how many seconds or beats lie
/// between two of its lines, and which of the two they are.
pub const GRIDS: [(&str, f64, bool); 10] = [
    ("1 s", 1.0, false),
    ("1/2 s", 0.5, false),
    ("1/4 s", 0.25, false),
    ("1/10 s", 0.1, false),
    ("1 beat", 1.0, true),
    ("1/2 beat", 0.5, true),
    ("1/3 beat", 1.0 / 3.0, true),
    ("1/4 beat", 0.25, true),
    ("1/8 beat", 0.125, true),
    ("1/16 beat", 0.0625, true),
];
/// The one element of a curve track's group.
const VALUE: &str = "value";
/// The element of every track's group that says whether a recording into it runs: 1 or 0.
pub(crate) const RECORDING: &str = "recording";

/// A track: its name, which is its variable group's, and what it holds. Its times are beats of
/// `clock.bpm`, so a change of the tempo moves them all together.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Track {
    pub name: String,
    /// Its place among the tracks, the lowest first: the order a panel draws them in.
    pub order: u32,
    /// What a recording into the track takes in: for notes the variable group of a MIDI device,
    /// for a curve an expression, as a param's. Empty is none.
    pub input: String,
    #[serde(flatten)]
    pub lanes: Lanes,
}

/// What a track holds: notes, with the keyboard a panel draws them on — its lowest and highest
/// pitch, the scale and root whose notes it marks, and the grid a gesture snaps to — or one curve through its keyframes, in
/// time order, over a range. The curve's value holds outside the keyframes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Lanes {
    Note {
        #[serde(default = "keyboard")]
        range: [u8; 2],
        #[serde(default = "chromatic")]
        scale: String,
        #[serde(default = "c")]
        root: String,
        #[serde(default = "quarter")]
        grid: String,
        #[serde(default)]
        notes: Vec<Note>,
    },
    Curve {
        #[serde(default = "unit")]
        range: [f64; 2],
        /// The most samples a second a recording takes of the input.
        #[serde(default = "thirty")]
        rate: f64,
        #[serde(default)]
        keys: Vec<Key>,
    },
}

/// A note: when it starts and how long it lasts, its MIDI pitch, and its velocity in 0..1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Note {
    pub at: f64,
    pub length: f64,
    pub pitch: u8,
    pub velocity: f64,
}

fn unit() -> [f64; 2] {
    [0.0, 1.0]
}

fn thirty() -> f64 {
    30.0
}

/// Two octaves from C3.
fn keyboard() -> [u8; 2] {
    [48, 72]
}

fn chromatic() -> String {
    scale::PRESET_NAMES[0].into()
}

fn c() -> String {
    scale::NOTES[0].into()
}

fn quarter() -> String {
    GRIDS[7].0.into()
}

/// A keyframe: the value at `at`, and the curve from it to the next one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Key {
    pub at: f64,
    pub value: f64,
    #[serde(default)]
    pub curve: Curve,
}

crate::machine::named!(Track);

impl Track {
    /// Whether the record holds together.
    pub fn check(&self) -> Result<(), String> {
        let time = |at: f64| at.is_finite() && at >= 0.0;
        let (ok, why) = match &self.lanes {
            Lanes::Note { range, scale, root, grid, notes } => {
                let whole = |n: &Note| time(n.at) && n.length.is_finite() && n.length > 0.0 && n.pitch <= 127 && (0.0..=1.0).contains(&n.velocity);
                let keyboard = range[0] < range[1] && range[1] <= 127 && scale::PRESET_NAMES.contains(&scale.as_str()) && scale::NOTES.contains(&root.as_str());
                let gridded = GRIDS.iter().any(|(name, ..)| name == grid);
                (keyboard && gridded && notes.iter().all(whole), "its keyboard shows a lower pitch to a higher one, 0..127, in a scale and a root a `Quantize` node offers; its grid is one a panel offers; a note starts at 0 or later, lasts more than 0, has a pitch in 0..127 and a velocity in 0..1")
            }
            Lanes::Curve { range, rate, keys } => {
                let whole = keys.iter().all(|k| time(k.at) && k.value.is_finite() && k.curve.check().is_ok()) && keys.windows(2).all(|k| k[0].at < k[1].at);
                (whole && range.iter().all(|v| v.is_finite()) && (1.0..=120.0).contains(rate), "a curve has a finite range, a rate of 1..120 samples a second and keyframes of finite numbers, each later than the one before")
            }
        };
        ok.then_some(()).ok_or_else(|| format!("track `{}`: {why}", self.name))?;
        // What it records from: a device's group by name, or an expression as a param's, which
        // has no `me` to read.
        let heard = match &self.lanes {
            _ if self.input.is_empty() => true,
            Lanes::Note { .. } => goofi_core::variables::is_valid_identifier(&self.input),
            Lanes::Curve { .. } => input_refs(&self.input).is_ok(),
        };
        heard.then_some(()).ok_or_else(|| format!("track `{}`: a note track's `input` is the group of a MIDI device, a curve track's an expression as a param's, without `me`", self.name))
    }

    /// Take a recording in. Notes are added to those the track holds. Each pass of samples
    /// becomes the keyframes that follow it, in place of those the track held in the pass's
    /// stretch, to within a hundredth of the track's range, which grows to hold what came. A
    /// take that leaves the curve more than [`MOST`] keyframes is refused, and the track is as
    /// it was.
    pub fn merge(&mut self, take: &Take) -> Result<(), String> {
        match (&mut self.lanes, take) {
            (Lanes::Note { range, notes, .. }, Take::Notes(taken)) => {
                *range = taken.iter().fold(*range, |r, n| [r[0].min(n.pitch), r[1].max(n.pitch)]);
                notes.extend(taken.iter().cloned());
            }
            (Lanes::Curve { range, keys, .. }, Take::Samples(passes)) => {
                let wide = passes.iter().flatten().fold(*range, |r, s| [r[0].min(s.value), r[1].max(s.value)]);
                let refused = || format!("track `{}`: the take leaves the curve more than {MOST} keyframes, the most it holds: record a shorter stretch, a slower rate or a smoother input", self.name);
                let mut merged = keys.clone();
                for pass in passes.iter().filter(|pass| !pass.is_empty()) {
                    let fitted = fit(pass, (wide[1] - wide[0]) / 100.0).ok_or_else(refused)?;
                    let (from, to) = (pass[0].at, pass[pass.len() - 1].at);
                    merged.retain(|k| k.at < from || k.at > to);
                    merged.extend(fitted);
                    merged.sort_by(|a, b| a.at.total_cmp(&b.at));
                }
                if merged.len() > MOST {
                    return Err(refused());
                }
                (*range, *keys) = (wide, merged);
            }
            _ => {}
        }
        Ok(())
    }

    /// The expression a recording into the track evaluates: a curve track's input, where it has one.
    pub fn expression(&self) -> Option<&str> {
        (matches!(self.lanes, Lanes::Curve { .. }) && !self.input.is_empty()).then_some(self.input.as_str())
    }

    /// The variables the track's group holds, each born at rest: what it plays, and whether a
    /// recording into it runs.
    pub fn entries(&self) -> Keyed {
        let recording = (RECORDING.to_string(), (format!("{}.{RECORDING}", self.name), Data::number(0.0), None));
        let mut held = self.played();
        held.push(recording);
        held
    }

    fn played(&self) -> Keyed {
        match &self.lanes {
            Lanes::Note { .. } => vec![(NOTES.0.into(), (format!("{}.{}", self.name, NOTES.0), Data::numbers(std::iter::repeat_n(0.0, NOTES.1)), None))],
            Lanes::Curve { range, keys, .. } => {
                let kind = AttributeKind::Num { vmin: range[0], vmax: range[1], int: false, face: Face::Plain };
                let born = Data::number(keys.last().map_or(range[0], |k| k.value));
                vec![(VALUE.into(), (format!("{}.{VALUE}", self.name), born.clone(), Some(kind.control(&born))))]
            }
        }
    }
}

/// The variables a curve track's input reads: an expression as a param's, but with no node of
/// its own, so no `me`. A playhead's element is the variable it is published as.
pub fn input_refs(text: &str) -> Result<(String, Vec<crate::expr_rewrite::VarRef>), goofi_node::ExprError> {
    let (rewritten, mut refs) = crate::expr_rewrite::rewrite(text)?;
    for r in &mut refs {
        match &r.target {
            Target::MeOut { .. } | Target::MeParam { .. } => return Err(goofi_node::ExprError("a track has no `me`".into())),
            Target::PlayheadField { playhead, field } => r.target = Target::Variable { key: format!("{playhead}.{field}") },
            _ => {}
        }
    }
    Ok((rewritten, refs))
}

/// The most keyframes a recording leaves a curve with. A curve is one functional that a reader
/// compiles whole: one of 4000 fitted keyframes played in three probe runs, and one of 8000 did
/// not in two.
pub const MOST: usize = 2000;

/// One sample of a curve's input: the beat it was taken at, the value, and whether the input
/// held that value until the next sample, a step, where it did not move there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub at: f64,
    pub value: f64,
    pub held: bool,
}

/// What a recording into a track took: notes, or the samples of its input, pass by pass. A
/// position that went back or was put elsewhere, as a loop and a seek do, begins another pass.
#[derive(Debug, PartialEq)]
pub enum Take {
    Notes(Vec<Note>),
    Samples(Vec<Vec<Sample>>),
}

/// A recording into a track, while it runs.
#[derive(Default)]
pub(crate) struct Recording {
    /// The keys held down, by their place in the device's `notes`: the beat each went down at,
    /// and how hard.
    held: HashMap<usize, (f64, f64)>,
    notes: Vec<Note>,
    passes: Vec<Vec<Sample>>,
    /// The last pass is ended: the next sample begins another.
    ended: bool,
    /// The stamp of the node frame the last sample is of.
    frame: Option<f64>,
    /// The last look: its beat, and the value the input had there, taken or not.
    last: f64,
    latest: Option<f64>,
}

impl Recording {
    /// The shortest a recorded note is: one whose key went down and up at one beat.
    const SHORTEST: f64 = 0.01;

    /// End the pass at the last look: every key still down ends there, and the input's value
    /// there is the pass's last sample. What comes next is another pass.
    pub(crate) fn cut(&mut self) {
        let (at, held) = (self.last, std::mem::take(&mut self.held));
        self.notes.extend(held.into_iter().map(|(key, down)| Self::note(key, down, at)));
        if let (Some(value), false, Some(pass)) = (self.latest.take(), self.ended, self.passes.last_mut()) {
            match pass.last_mut() {
                Some(taken) if at > taken.at => pass.push(Sample { at, value, held: false }),
                Some(taken) => taken.value = value,
                None => {}
            }
        }
        (self.ended, self.frame) = (true, None);
    }

    /// The device's keys at the beat `at`: one that went down begins a note, one that came up
    /// ends it.
    pub(crate) fn keys(&mut self, at: f64, velocities: impl Iterator<Item = f64>) {
        if at < self.last {
            self.cut();
        }
        self.last = at;
        for (key, velocity) in velocities.enumerate() {
            match (self.held.get(&key).copied(), velocity > 0.0) {
                (None, true) => drop(self.held.insert(key, (at, velocity))),
                (Some(down), false) => {
                    self.held.remove(&key);
                    self.notes.push(Self::note(key, down, at));
                }
                _ => {}
            }
        }
    }

    /// The input's value at the beat `at`, and `frame`, the stamp of the newest node frame it
    /// read. A sample is taken where the input is new and `every` beats have gone by since the
    /// last one. An input that reads a node is new with each frame of the node, so a node slower
    /// than the rate gives a sample a frame. Any other is new when its value is another; where a
    /// look between found it the same, it held that value until now, a step. One beat holds one
    /// value, the newest.
    pub(crate) fn sample(&mut self, at: f64, value: f64, frame: Option<f64>, every: f64) {
        if at < self.last {
            self.cut();
        }
        let looked = std::mem::replace(&mut self.last, at);
        if !value.is_finite() {
            return;
        }
        let before = self.latest.replace(value);
        if std::mem::take(&mut self.ended) || self.passes.is_empty() {
            self.passes.push(Vec::new());
        }
        let pass = self.passes.last_mut().expect("a pass");
        let Some(taken) = pass.last_mut() else {
            self.frame = frame;
            return pass.push(Sample { at, value, held: false });
        };
        if at == taken.at {
            (taken.value, self.frame) = (value, frame);
        } else if at >= taken.at + every && (if frame.is_some() { frame != self.frame } else { value != taken.value }) {
            taken.held = frame.is_none() && looked > taken.at && before == Some(taken.value);
            self.frame = frame;
            pass.push(Sample { at, value, held: false });
        }
    }

    /// End at the beat `at`, which is the last look.
    pub(crate) fn end(mut self, at: f64, notes: bool) -> Take {
        self.last = at.max(self.last);
        self.cut();
        if notes { Take::Notes(self.notes) } else { Take::Samples(self.passes) }
    }

    /// A key of the device as a note: its place there is 128 pitches a channel.
    fn note(key: usize, (down, velocity): (f64, f64), up: f64) -> Note {
        Note { at: down, length: (up - down).max(Self::SHORTEST), pitch: (key % 128) as u8, velocity: velocity.clamp(0.0, 1.0) }
    }
}

/// The keyframes that follow one pass of a recording to within `tolerance`: as few as do it,
/// each eased to the next by one cubic bezier, or held until the next where the input held.
/// None where that is more than [`MOST`], which no curve holds.
fn fit(pass: &[Sample], tolerance: f64) -> Option<Vec<Key>> {
    let mut keys = Vec::new();
    // A held sample ends a run: nothing is fitted across a step.
    for run in pass.chunk_by(|before, _| !before.held) {
        spans(run, tolerance, &mut keys);
        let last = run[run.len() - 1];
        keys.push(Key { at: last.at, value: last.value, curve: if last.held { Curve::Step } else { Curve::Linear } });
        if keys.len() > MOST {
            return None;
        }
    }
    Some(keys)
}

/// The keyframes of a run of samples, but for its last sample. Between two samples the take is
/// the straight line between them, and a cubic is judged against that line all along a stretch,
/// not at the samples alone. Where the cubic nearest a stretch is off by more than the tolerance,
/// the sample nearest the worst of it becomes a keyframe, and each half is fitted again.
fn spans(run: &[Sample], tolerance: f64, keys: &mut Vec<Key>) {
    /// The points of each line between two samples that a cubic is held to.
    const POINTS: usize = 4;
    // A cubic from (0, 0) to (1, 1) with its handles a third and two thirds along is a cubic in
    // the progress itself, so the two handle heights nearest the take are a least squares.
    let basis = |x: f64| (3.0 * (1.0 - x) * (1.0 - x) * x, 3.0 * (1.0 - x) * x * x, x * x * x);
    // The stretches still to fit, the earliest on top, each from one sample to a later one.
    let mut open = vec![(0, run.len() - 1)];
    while let Some((from, to)) = open.pop() {
        // Past the most a curve holds, the rest is not fitted: the caller refuses the take.
        if to <= from || keys.len() > MOST {
            continue;
        }
        let (first, last) = (run[from], run[to]);
        let (dt, dv) = (last.at - first.at, last.value - first.value);
        // Each point by the sample it lies nearest, its beat and the take's value there.
        let line = (from..to).flat_map(|i| (0..POINTS).map(move |k| (i, k as f64 / POINTS as f64)));
        let take = || line.clone().map(|(i, f)| (i + usize::from(f >= 0.5), run[i].at + (run[i + 1].at - run[i].at) * f, run[i].value + (run[i + 1].value - run[i].value) * f));
        let level = dv.abs() <= tolerance / 100.0;
        let (mut a, mut b, mut c, mut p, mut q) = (0.0, 0.0, 0.0, 0.0, 0.0);
        if !level && to > from + 1 {
            for (_, t, v) in take() {
                let (b1, b2, rest) = basis((t - first.at) / dt);
                let want = (v - first.value) / dv - rest;
                (a, b, c, p, q) = (a + b1 * b1, b + b1 * b2, c + b2 * b2, p + b1 * want, q + b2 * want);
            }
        }
        let det = a * c - b * b;
        let (y1, y2) = if det.abs() > 1e-12 { ((p * c - q * b) / det, (q * a - p * b) / det) } else { (1.0 / 3.0, 2.0 / 3.0) };
        let off = |(i, t, v): (usize, f64, f64)| {
            let (b1, b2, rest) = basis((t - first.at) / dt);
            (i, (first.value + dv * (b1 * y1 + b2 * y2 + rest) - v).abs())
        };
        let (worst, error) = take().map(off).fold((from, 0.0), |far, now| if now.1 > far.1 { now } else { far });
        if error <= tolerance || to == from + 1 {
            let straight = (y1 - 1.0 / 3.0).abs() < 1e-6 && (y2 - 2.0 / 3.0).abs() < 1e-6;
            let curve = if straight { Curve::Linear } else { Curve::Custom(vec![[0.0, 0.0, 0.0, 0.0, 1.0 / 3.0, y1], [1.0, 1.0, -1.0 / 3.0, y2 - 1.0, 0.0, 0.0]]) };
            keys.push(Key { at: first.at, value: first.value, curve });
            continue;
        }
        let cut = worst.clamp(from + 1, to - 1);
        open.push((cut, to));
        open.push((from, cut));
    }
}

/// The timeline as it runs: what every track writes at a position, built whole from the tracks
/// and the tempo. A curve is compiled here once; a note is `[start, end, pitch, velocity]` in seconds.
#[derive(Default)]
pub(crate) struct Timeline {
    curves: Vec<(String, Data)>,
    /// Per note track, by its name.
    notes: Vec<(String, Vec<[f64; 4]>)>,
}

impl Timeline {
    pub(crate) fn new(tracks: &IndexMap<String, Track>, bpm: f64, evaluator: &Option<Arc<dyn ExprEvaluator>>) -> Timeline {
        let mut out = Timeline::default();
        for track in tracks.values() {
            // A tempo of nothing is the slowest there is: a beat never comes.
            let seconds = |at: f64| at * 60.0 / bpm.max(1e-9);
            match &track.lanes {
                Lanes::Note { notes, .. } => {
                    let notes = notes.iter().map(|n| [seconds(n.at), seconds(n.at + n.length), f64::from(n.pitch), n.velocity]).collect();
                    out.notes.push((track.name.clone(), notes));
                }
                Lanes::Curve { range, keys, .. } => {
                    let leg = |k: &[Key]| Leg { start: seconds(k[0].at), end: seconds(k[1].at), from: Data::number(k[0].value), to: Data::number(k[1].value), curve: k[0].curve.clone() };
                    let motion = Motion { legs: keys.windows(2).map(leg).collect(), rest: Data::number(keys.last().map_or(range[0], |k| k.value)) };
                    let value = motion.source().map_or(motion.rest.clone(), |source| Data::functional(functional_code(evaluator, &source), Meta::default()));
                    out.curves.push((format!("{}.{VALUE}", track.name), value));
                }
            }
        }
        out
    }

    /// What every track's group holds at the position `now`.
    pub(crate) fn writes(&self, now: f64) -> Vec<(String, Data)> {
        let held = self.notes.iter().map(|(name, notes)| {
            let mut velocities = vec![0.0; NOTES.1];
            for [_, _, pitch, velocity] in notes.iter().filter(|n| (n[0]..n[1]).contains(&now)) {
                velocities[*pitch as usize] = *velocity;
            }
            (format!("{name}.{}", NOTES.0), Data::numbers(velocities))
        });
        self.curves.iter().cloned().chain(held).collect()
    }

    /// The notes the position `now` is in: each by its track, its place in the track, its pitch
    /// and its velocity.
    pub(crate) fn held(&self, now: f64) -> Vec<(String, usize, u8, f64)> {
        let held = |(track, notes): &(String, Vec<[f64; 4]>)| {
            let inside = notes.iter().enumerate().filter(|(_, n)| (n[0]..n[1]).contains(&now));
            inside.map(|(i, n)| (track.clone(), i, n[2] as u8, n[3])).collect::<Vec<_>>()
        };
        self.notes.iter().flat_map(held).collect()
    }

    /// The next position past `now` at which a note starts or ends.
    pub(crate) fn next_deadline(&self, now: f64) -> Option<f64> {
        self.notes.iter().flat_map(|(_, notes)| notes.iter().flat_map(|n| [n[0], n[1]])).filter(|at| *at > now).reduce(f64::min)
    }
}
