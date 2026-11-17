//! The timeline: tracks of notes and of keyframed curves on the patch clock's position. A track
//! is a group of variables: a curve attribute is ONE functional of `t` over the whole track, and
//! a note track is the velocities held at the position.

use std::sync::Arc;

use goofi_core::ease::Curve;
use goofi_core::variables::{is_valid_identifier, Entries, MIDI_ENTRIES};
use goofi_core::{Data, Face, Meta};
use goofi_node::ExprEvaluator;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::machine::{functional_code, AttributeKind, Leg, Motion, NAME_RULE};

/// The one element of a note track's group: each held note's velocity, where a MIDI device's
/// `notes` has its channel 1, so a reader of a device reads a track.
pub const NOTES: &str = "notes";

/// What a track's times are in: seconds, or beats of `clock.bpm`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    #[default]
    Seconds,
    Beats,
}

/// A track: its name, which is its variable group's, the unit of its times, and what it holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Track {
    pub name: String,
    #[serde(default)]
    pub unit: Unit,
    #[serde(flatten)]
    pub lanes: Lanes,
}

/// What a track holds: notes, or attributes by id, each a curve through its keyframes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Lanes {
    Note {
        #[serde(default)]
        notes: Vec<Note>,
    },
    Curve {
        #[serde(default)]
        attributes: IndexMap<String, Lane>,
    },
}

/// A note: when it starts and how long it lasts, in the track's unit; its MIDI pitch; its
/// velocity in 0..1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Note {
    pub at: f64,
    pub length: f64,
    pub pitch: u8,
    pub velocity: f64,
}

/// A curve attribute: the range its values take, and its keyframes in time order. Its value holds
/// before the first keyframe and after the last.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Lane {
    pub name: String,
    pub range: [f64; 2],
    #[serde(default)]
    pub keys: Vec<Key>,
}

/// A keyframe: the value at `at`, in the track's unit, and the curve from it to the next one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct Key {
    pub at: f64,
    pub value: f64,
    #[serde(default)]
    pub curve: Curve,
}

crate::machine::named!(Track, Lane);

impl Track {
    /// Whether the record holds together.
    pub fn check(&self) -> Result<(), String> {
        let time = |at: f64| at.is_finite() && at >= 0.0;
        match &self.lanes {
            Lanes::Note { notes } if notes.iter().any(|n| !time(n.at) || !(n.length.is_finite() && n.length > 0.0) || n.pitch > 127 || !(0.0..=1.0).contains(&n.velocity)) => {
                Err("a note starts at 0 or later, lasts more than 0, has a pitch in 0..127 and a velocity in 0..1".into())
            }
            Lanes::Note { .. } => Ok(()),
            Lanes::Curve { attributes } => attributes.iter().try_for_each(|(id, lane)| {
                let named = !is_valid_identifier(id) && is_valid_identifier(&lane.name) && attributes.values().filter(|l| l.name == lane.name).count() == 1;
                let keyed = lane.keys.iter().all(|k| time(k.at) && k.value.is_finite() && k.curve.check().is_ok()) && lane.keys.windows(2).all(|k| k[0].at < k[1].at);
                if named && keyed && lane.range.iter().all(|v| v.is_finite()) {
                    return Ok(());
                }
                Err(format!("attribute `{}`: a name of its own ({NAME_RULE}), a finite range, and keyframes of finite numbers, each later than the one before", lane.name))
            }),
        }
    }

    /// The elements of the track's group, each by the key a rename is read off and its name.
    pub fn elements(&self) -> Vec<(String, String)> {
        match &self.lanes {
            Lanes::Note { .. } => vec![(NOTES.into(), NOTES.into())],
            Lanes::Curve { attributes } => attributes.iter().map(|(id, lane)| (id.clone(), lane.name.clone())).collect(),
        }
    }

    /// The variables the track's group holds, each born at rest.
    pub fn entries(&self) -> Entries {
        match &self.lanes {
            Lanes::Note { .. } => vec![(format!("{}.{NOTES}", self.name), Data::numbers(std::iter::repeat_n(0.0, MIDI_ENTRIES[1].1)), None)],
            Lanes::Curve { attributes } => attributes.values().map(|lane| {
                let kind = AttributeKind::Num { vmin: lane.range[0], vmax: lane.range[1], int: false, face: Face::Plain };
                let born = Data::number(lane.keys.first().map_or(lane.range[0], |k| k.value));
                (format!("{}.{}", self.name, lane.name), born.clone(), Some(kind.control(&born)))
            }).collect(),
        }
    }
}

/// The timeline as it runs: what every track writes at a position, built whole from the tracks
/// and the tempo. A curve is compiled here once; a note is `[start, end, pitch, velocity]` in seconds.
#[derive(Default)]
pub struct Timeline {
    curves: Vec<(String, Data)>,
    notes: Vec<(String, Vec<[f64; 4]>)>,
}

impl Timeline {
    pub fn new(tracks: &IndexMap<String, Track>, bpm: f64, evaluator: &Option<Arc<dyn ExprEvaluator>>) -> Timeline {
        let mut out = Timeline::default();
        for track in tracks.values() {
            let seconds = |at: f64| if track.unit == Unit::Beats { at * 60.0 / bpm } else { at };
            match &track.lanes {
                Lanes::Note { notes } => {
                    let notes = notes.iter().map(|n| [seconds(n.at), seconds(n.at + n.length), f64::from(n.pitch), n.velocity]).collect();
                    out.notes.push((format!("{}.{NOTES}", track.name), notes));
                }
                Lanes::Curve { attributes } => {
                    for lane in attributes.values() {
                        let leg = |k: &[Key]| Leg { start: seconds(k[0].at), end: seconds(k[1].at), from: Data::number(k[0].value), to: Data::number(k[1].value), curve: k[0].curve };
                        let motion = Motion { legs: lane.keys.windows(2).map(leg).collect(), rest: Data::number(lane.keys.last().map_or(lane.range[0], |k| k.value)) };
                        let value = motion.source().map_or(motion.rest.clone(), |source| Data::functional(functional_code(evaluator, &source), Meta::default()));
                        out.curves.push((format!("{}.{}", track.name, lane.name), value));
                    }
                }
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.curves.is_empty() && self.notes.is_empty()
    }

    /// What every track's group holds at the position `now`.
    pub fn writes(&self, now: f64) -> Vec<(String, Data)> {
        let held = self.notes.iter().map(|(name, notes)| {
            let mut velocities = vec![0.0; MIDI_ENTRIES[1].1];
            for [_, _, pitch, velocity] in notes.iter().filter(|n| (n[0]..n[1]).contains(&now)) {
                velocities[*pitch as usize] = *velocity;
            }
            (name.clone(), Data::numbers(velocities))
        });
        self.curves.iter().cloned().chain(held).collect()
    }

    /// The next position past `now` at which a note starts or ends.
    pub fn next_deadline(&self, now: f64) -> Option<f64> {
        self.notes.iter().flat_map(|(_, notes)| notes.iter().flat_map(|n| [n[0], n[1]])).filter(|at| *at > now).reduce(f64::min)
    }
}
