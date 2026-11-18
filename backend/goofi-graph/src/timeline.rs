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
        // What it records from: a device's group by name, or an expression that reads no machine.
        let free = |refs: &[crate::expr_rewrite::VarRef]| !refs.iter().any(|r| matches!(r.target, Target::Playhead | Target::State { .. }));
        let heard = match &self.lanes {
            _ if self.input.is_empty() => true,
            Lanes::Note { .. } => goofi_core::variables::is_valid_identifier(&self.input),
            Lanes::Curve { .. } => crate::expr_rewrite::rewrite_machine(&self.input).is_ok_and(|(_, refs)| free(&refs)),
        };
        heard.then_some(()).ok_or_else(|| format!("track `{}`: a note track's `input` is the group of a MIDI device, a curve track's an expression that reads no playhead and no state", self.name))
    }

    /// Take a recording in. Notes are added to those the track holds. Each pass of samples
    /// becomes the keyframes that follow it to within a hundredth of what the take spans, in
    /// place of those the track held in the pass's stretch. The track's range grows to hold
    /// what came.
    pub fn merge(&mut self, take: Take) {
        match (&mut self.lanes, take) {
            (Lanes::Note { range, notes, .. }, Take::Notes(taken)) => {
                *range = taken.iter().fold(*range, |r, n| [r[0].min(n.pitch), r[1].max(n.pitch)]);
                notes.extend(taken);
            }
            (Lanes::Curve { range, keys, .. }, Take::Samples(samples)) => {
                let [low, high] = samples.iter().fold([f64::INFINITY, f64::NEG_INFINITY], |[low, high], s| [low.min(s.1), high.max(s.1)]);
                for pass in samples.chunk_by(|a, b| a.0 < b.0) {
                    let (from, to) = (pass[0].0, pass[pass.len() - 1].0);
                    keys.retain(|k| k.at < from || k.at > to);
                    keys.extend(fit(pass, (high - low) / 100.0));
                    keys.sort_by(|a, b| a.at.total_cmp(&b.at));
                    *range = [range[0].min(low), range[1].max(high)];
                }
            }
            _ => {}
        }
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

/// What a recording into a track took: notes, or samples of its input, each with the beat it
/// was taken at. Samples whose beat goes back begin another pass, as a clock that loops makes.
#[derive(Debug, PartialEq)]
pub enum Take {
    Notes(Vec<Note>),
    Samples(Vec<(f64, f64)>),
}

/// A recording into a track, while it runs.
#[derive(Default)]
pub(crate) struct Recording {
    /// The keys held down, by their place in the device's `notes`: the beat each went down at,
    /// and how hard.
    held: HashMap<usize, (f64, f64)>,
    notes: Vec<Note>,
    samples: Vec<(f64, f64)>,
    /// The stamp of the node frame the last sample is of, the input's newest value, taken or
    /// not, and the last beat seen.
    frame: Option<f64>,
    latest: Option<f64>,
    last: f64,
}

impl Recording {
    /// The shortest a recorded note is: one whose key went down and up at one beat.
    const SHORTEST: f64 = 0.01;

    /// The device's keys at the beat `at`: one that went down begins a note, one that came up
    /// ends it. A position that went back ends every note where it was.
    pub(crate) fn keys(&mut self, at: f64, velocities: impl Iterator<Item = f64>) {
        if at < self.last {
            self.lift(self.last);
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
    /// last one: an input that reads a node is new with each frame of the node, so a node slower
    /// than the rate gives a sample a frame; any other is new when its value is another, and
    /// held what it had until then. One beat holds one value, the newest, and a position that
    /// went back begins another pass.
    pub(crate) fn sample(&mut self, at: f64, value: f64, frame: Option<f64>, every: f64) {
        if !value.is_finite() {
            return;
        }
        self.latest = Some(value);
        self.last = at;
        let Some(taken) = self.samples.last_mut().filter(|taken| at >= taken.0) else {
            self.frame = frame;
            return self.samples.push((at, value));
        };
        if at == taken.0 {
            (taken.1, self.frame) = (value, frame);
        } else if at >= taken.0 + every && (if frame.is_some() { frame != self.frame } else { value != taken.1 }) {
            let held = (frame.is_none() && at - every > taken.0 + every).then_some((at - every, taken.1));
            self.frame = frame;
            self.samples.extend(held.into_iter().chain([(at, value)]));
        }
    }

    /// End at the beat `at`: every key still down ends there, and the input's newest value is
    /// the curve's there, whatever the rate.
    pub(crate) fn end(mut self, at: f64, notes: bool) -> Take {
        let at = at.max(self.last);
        self.lift(at);
        match (self.latest, self.samples.last_mut()) {
            (Some(value), Some(taken)) if at > taken.0 => self.samples.push((at, value)),
            (Some(value), Some(taken)) => taken.1 = value,
            _ => {}
        }
        if notes { Take::Notes(self.notes) } else { Take::Samples(self.samples) }
    }

    fn lift(&mut self, at: f64) {
        let held: Vec<_> = self.held.drain().collect();
        self.notes.extend(held.into_iter().map(|(key, down)| Self::note(key, down, at)));
    }

    /// A key of the device as a note: its place there is 128 pitches a channel.
    fn note(key: usize, (down, velocity): (f64, f64), up: f64) -> Note {
        Note { at: down, length: (up - down).max(Self::SHORTEST), pitch: (key % 128) as u8, velocity: velocity.clamp(0.0, 1.0) }
    }
}

/// The keyframes that follow the samples `(at, value)` of a recording, in time order, to within
/// `tolerance` of every value: as few as do it, each eased to the next by one cubic bezier.
/// Between two keyframes the curve is the cubic nearest the samples there; where that is off by
/// more than the tolerance, the sample furthest off becomes a keyframe, and each half is fitted
/// again.
pub fn fit(samples: &[(f64, f64)], tolerance: f64) -> Vec<Key> {
    let Some(last) = samples.last() else { return Vec::new() };
    let mut keys = Vec::new();
    span(samples, tolerance, &mut keys);
    keys.push(Key { at: last.0, value: last.1, curve: Curve::Linear });
    keys
}

/// The keyframes of one stretch of samples, its last sample left to the stretch after it.
fn span(samples: &[(f64, f64)], tolerance: f64, keys: &mut Vec<Key>) {
    let (Some(&(t0, v0)), Some(&(t1, v1))) = (samples.first(), samples.last()) else { return };
    if samples.len() < 2 {
        return;
    }
    let (dt, dv) = (t1 - t0, v1 - v0);
    // A cubic from (0, 0) to (1, 1) with its handles a third and two thirds along is a cubic in
    // the progress itself, so the two handle heights nearest the samples are a least squares.
    let basis = |x: f64| (3.0 * (1.0 - x) * (1.0 - x) * x, 3.0 * (1.0 - x) * x * x, x * x * x);
    let progress = |t: f64| (t - t0) / dt;
    let level = dv.abs() <= tolerance;
    let (mut a, mut b, mut c, mut p, mut q) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for &(t, v) in samples.iter().filter(|_| !level) {
        let (b1, b2, rest) = basis(progress(t));
        let want = (v - v0) / dv - rest;
        (a, b, c, p, q) = (a + b1 * b1, b + b1 * b2, c + b2 * b2, p + b1 * want, q + b2 * want);
    }
    let det = a * c - b * b;
    let (y1, y2) = if det.abs() > 1e-12 { ((p * c - q * b) / det, (q * a - p * b) / det) } else { (1.0 / 3.0, 2.0 / 3.0) };
    let off = |&(t, v): &(f64, f64)| {
        let (b1, b2, rest) = basis(progress(t));
        (v0 + dv * (b1 * y1 + b2 * y2 + rest) - v).abs()
    };
    let (worst, error) = samples.iter().map(off).enumerate().fold((0, 0.0), |far, now| if now.1 > far.1 { now } else { far });
    if error <= tolerance || samples.len() == 2 {
        let straight = level || ((y1 - 1.0 / 3.0).abs() < 1e-6 && (y2 - 2.0 / 3.0).abs() < 1e-6);
        let curve = if straight { Curve::Linear } else { Curve::Custom(vec![[0.0, 0.0, 0.0, 0.0, 1.0 / 3.0, y1], [1.0, 1.0, -1.0 / 3.0, y2 - 1.0, 0.0, 0.0]]) };
        keys.push(Key { at: t0, value: v0, curve });
        return;
    }
    let cut = worst.clamp(1, samples.len() - 2);
    span(&samples[..=cut], tolerance, keys);
    span(&samples[cut..], tolerance, keys);
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
