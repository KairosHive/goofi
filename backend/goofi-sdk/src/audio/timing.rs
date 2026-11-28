//! A time a node takes in seconds, in beats of the patch's tempo, or as a note division: one
//! rule for every node that has a time, so each offers the same three units, the same divisions
//! and the same tempo. A node declares the five params with the functions here, in this order,
//! and reads the time with [`seconds`].

use crate::audio::{ExprDecl, ExprMode, Face, ParamDecl, ParamSpec, Show};

/// What a time is counted in, in the order the `unit` param offers them.
pub const UNITS: &[&str] = &["seconds", "beats", "division"];

/// The note divisions, each as beats: whole beats and their parts, a dotted one half as long
/// again, a third and a sixth for triplets.
pub const DIVISIONS: &[(&str, f32)] = &[
    ("4", 4.0),
    ("2", 2.0),
    ("1", 1.0),
    ("1/2", 0.5),
    ("1/3", 1.0 / 3.0),
    ("1/4", 0.25),
    ("1/6", 1.0 / 6.0),
    ("1/8", 0.125),
    ("1/16", 0.0625),
    ("2.", 3.0),
    ("1.", 1.5),
    ("1/2.", 0.75),
    ("1/4.", 0.375),
    ("1/8.", 0.1875),
];

/// The divisions by name, for the `division` param's options.
const NAMES: [&str; DIVISIONS.len()] = {
    let mut names = [""; DIVISIONS.len()];
    let mut i = 0;
    while i < DIVISIONS.len() {
        names[i] = DIVISIONS[i].0;
        i += 1;
    }
    names
};

/// The `unit` param named `name`: which of the three a time is counted in.
pub const fn unit(group: &'static str, name: &'static str) -> ParamDecl {
    ParamDecl {
        doc: Some("what the time is counted in: seconds, beats of the tempo, or a note division"),
        ..ParamDecl::new(group, name, ParamSpec::Str { default: UNITS[0], options: UNITS, refresh: false })
    }
}

/// The time in seconds, shown under the unit `seconds` of the param `unit`.
pub const fn in_seconds(group: &'static str, name: &'static str, unit: &'static Show, default: &'static [f64], max: f64, doc: &'static str) -> ParamDecl {
    ParamDecl {
        doc: Some(doc),
        show: std::slice::from_ref(unit),
        ..ParamDecl::new(group, name, ParamSpec::Num { default, min: 0.0, max, int: false, options: &[], face: Face::Plain })
    }
}

/// The time in beats, shown under the unit `beats`.
pub const fn in_beats(group: &'static str, name: &'static str, unit: &'static Show, default: &'static [f64], max: f64) -> ParamDecl {
    ParamDecl {
        doc: Some("the time in beats of the tempo"),
        show: std::slice::from_ref(unit),
        ..ParamDecl::new(group, name, ParamSpec::Num { default, min: 0.0, max, int: false, options: &[], face: Face::Plain })
    }
}

/// The time as a note division, shown under the unit `division`.
pub const fn in_division(group: &'static str, name: &'static str, unit: &'static Show, default: &'static str) -> ParamDecl {
    ParamDecl {
        doc: Some("the time as a note division, in beats: a dot is half as long again, a third and a sixth are triplets"),
        show: std::slice::from_ref(unit),
        ..ParamDecl::new(group, name, ParamSpec::Str { default, options: &NAMES, refresh: false })
    }
}

/// The tempo a time in beats or as a division is read by: the patch's own, unless it is set.
pub const fn tempo(group: &'static str, unit: &'static Show) -> ParamDecl {
    ParamDecl {
        expression: Some(ExprDecl { source: "vars.clock.bpm", mode: ExprMode::On }),
        doc: Some("beats a minute: the patch's tempo, `vars.clock.bpm`, unless another is set"),
        show: std::slice::from_ref(unit),
        ..ParamDecl::new(group, "tempo", ParamSpec::Num { default: &[120.0], min: 1.0, max: 999.0, int: false, options: &[], face: Face::Plain })
    }
}

/// The seconds the five params stand for at one sample: `unit` and `division` are their options'
/// indices, as a param hands a text.
pub fn seconds(unit: f32, seconds: f32, beats: f32, division: f32, tempo: f32) -> f32 {
    let beat = 60.0 / tempo.max(1.0);
    match unit as usize {
        0 => seconds,
        1 => beats * beat,
        _ => DIVISIONS[(division as usize).min(DIVISIONS.len() - 1)].1 * beat,
    }
}
