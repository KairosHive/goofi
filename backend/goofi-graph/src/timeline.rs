//! The timeline: tracks of notes and of keyframed curves on the patch clock's position. A track
//! is a group of variables: a curve track is ONE functional of `t` over the whole track, and a
//! note track is the velocities held at the position.

use std::sync::Arc;

use goofi_core::ease::Curve;
use goofi_core::variables::MIDI_ENTRIES;
use goofi_core::{Data, Face, Meta};
use goofi_node::ExprEvaluator;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::machine::{functional_code, AttributeKind, Keyed, Leg, Motion};

/// The one element of a note track's group, and its width: each held note's velocity, where a
/// MIDI device's `notes` has its channel 1, so a reader of a device reads a track.
const NOTES: (&str, usize) = MIDI_ENTRIES[1];
/// The one element of a curve track's group.
const VALUE: &str = "value";

/// A track: its name, which is its variable group's; whether its times are beats of `clock.bpm`
/// and not seconds; and what it holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Track {
    pub name: String,
    #[serde(default)]
    pub beats: bool,
    #[serde(flatten)]
    pub lanes: Lanes,
}

/// What a track holds: notes, or one curve through its keyframes, in time order, over a range.
/// The curve's value holds outside the keyframes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Lanes {
    Note {
        #[serde(default)]
        notes: Vec<Note>,
    },
    Curve {
        #[serde(default = "unit")]
        range: [f64; 2],
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
            Lanes::Note { notes } => {
                let whole = |n: &Note| time(n.at) && n.length.is_finite() && n.length > 0.0 && n.pitch <= 127 && (0.0..=1.0).contains(&n.velocity);
                (notes.iter().all(whole), "a note starts at 0 or later, lasts more than 0, has a pitch in 0..127 and a velocity in 0..1")
            }
            Lanes::Curve { range, keys } => {
                let whole = keys.iter().all(|k| time(k.at) && k.value.is_finite() && k.curve.check().is_ok()) && keys.windows(2).all(|k| k[0].at < k[1].at);
                (whole && range.iter().all(|v| v.is_finite()), "a curve has a finite range and keyframes of finite numbers, each later than the one before")
            }
        };
        ok.then_some(()).ok_or_else(|| format!("track `{}`: {why}", self.name))
    }

    /// The variables the track's group holds, each born at rest.
    pub fn entries(&self) -> Keyed {
        match &self.lanes {
            Lanes::Note { .. } => vec![(NOTES.0.into(), (format!("{}.{}", self.name, NOTES.0), Data::numbers(std::iter::repeat_n(0.0, NOTES.1)), None))],
            Lanes::Curve { range, keys } => {
                let kind = AttributeKind::Num { vmin: range[0], vmax: range[1], int: false, face: Face::Plain };
                let born = Data::number(keys.last().map_or(range[0], |k| k.value));
                vec![(VALUE.into(), (format!("{}.{VALUE}", self.name), born.clone(), Some(kind.control(&born))))]
            }
        }
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
            let seconds = |at: f64| if track.beats { at * 60.0 / bpm.max(1e-9) } else { at };
            match &track.lanes {
                Lanes::Note { notes } => {
                    let notes = notes.iter().map(|n| [seconds(n.at), seconds(n.at + n.length), f64::from(n.pitch), n.velocity]).collect();
                    out.notes.push((track.name.clone(), notes));
                }
                Lanes::Curve { range, keys } => {
                    let leg = |k: &[Key]| Leg { start: seconds(k[0].at), end: seconds(k[1].at), from: Data::number(k[0].value), to: Data::number(k[1].value), curve: k[0].curve };
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
