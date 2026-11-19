//! The timeline: tracks of notes and of keyframed curves on the patch clock, each a group of
//! variables. A track is named by id or by name, as a machine's records are.

use serde_json::{json, Value};

use super::machine::id_of;
use super::{op, EffectOp, NoArgs, ReadOp, WriteOp};
use crate::machines::Msg;
use crate::{AppState, Caller, Txn};
use goofi_graph::timeline::{Key, Lanes, Note, Track};

op!(List, "timeline list", 0, NoArgs,
    "Every track of the timeline, under its id, in the order a panel draws them. A track plays on the patch clock's position (`clock set`): its times are beats of `clock.bpm`. A `note` track holds `notes`, each `{at, length, pitch, velocity}`; the keyboard a panel draws them on is the panel's own. It writes `vars.<track>.notes`, the velocity of each held note in 0..1 as a MIDI device's `notes` has its channel 1. A `curve` track holds `range: [min, max]` and `keys: [{at, value, curve}]`, and writes `vars.<track>.value`: the value eases from each keyframe to the next by that keyframe's `curve`, and holds outside them. A `curve` is `smooth` (the default: through the keyframe at the slope its neighbours give it, level at a peak and in a valley, never past their values), or an attribute's `step`, `linear`, `easing` or `{custom: …}` (see `machine attribute add`). `input` is what `timeline track record` takes in; `vars.<track>.recording` is 1 while it does, and `vars.<track>.take` is what it has taken so far, at most 256 notes (`at, length, pitch, velocity`) or samples (`at, value`).",
    "{tracks: {id: {name, input, kind: note, notes} | {name, input, kind: curve, range, rate, keys}}}");

op!(TrackAdd, "timeline track add", 1, TrackAddArgs {
    pub kind: String,
    pub name: Option<String>,
},
    "Add a track of `kind` `note` or `curve` (see `timeline list`). Without a name, use the first free track0/track1/... name; the name is the track's variable group.",
    "{id, name} — the track as stored");

op!(TrackEdit, "timeline track edit", 1, TrackEditArgs {
    pub track: String,
    pub to: Option<String>,
    #[schemars(with = "Option<Value>")]
    pub notes: Option<Vec<Note>>,
    pub range: Option<[f64; 2]>,
    #[schemars(with = "Option<Value>")]
    pub keys: Option<Vec<Key>>,
    pub input: Option<String>,
    pub rate: Option<f64>,
},
    "Change a track. `to` renames it: its variable group moves with what it holds, and every expression that reads it is rewritten. `notes` replaces a note track's notes whole; `range` and `keys` replace a curve track's range and its keyframes whole. `input` is what a recording takes in (see `timeline track record`), an expression as a param's but with no `me`, and an empty one is none: a note track reads the keys it holds, a MIDI device's `vars.<group>.notes` (a `group` of `midi list`) or another note track's; a curve track reads its first number, such as `nd('lfo0')`, `vars.pads.cc[7]` or `sm('head').gain`. `rate` is the most samples a second a curve track's recording takes, 1..120. Previewable: a drag sends the notes or the keyframes as a preview.",
    "{name}");

op!(TrackRecord, "timeline track record", 1, TrackRecordArgs {
    pub track: String,
    pub on: bool,
},
    "Begin (`on` true) or end a recording of the track's `input` into the track, at the clock's position as it plays: the op leaves the clock alone, and a clock that loops records pass after pass. A note track takes each key its input holds, on any channel, from when it goes down to when it comes up; the end adds those notes to the notes it holds, as one undo step, and widens `range` to show them. A curve track takes its input's value at most `rate` times a second — an input that reads a node takes a sample for each frame of the node, so a slower node gives its own rate, and any other input holds its value between two changes, a `step` — and the end puts keyframes that follow the take to within a hundredth of the track's `range`, each eased to the next by a fitted bezier `curve`, in place of the keyframes in the stretch each pass covered. A take that leaves the curve more than two thousand keyframes is refused whole: the op answers the refusal, and the track stays as it was. A position that goes back or is put elsewhere begins another pass, so a seek leaves what it jumped over as it was. `vars.<track>.recording` is 1 while a recording runs. Refused to begin without an `input`.",
    "{name, recording}");

op!(TrackMove, "timeline track move", 1, TrackMoveArgs {
    pub track: String,
    pub before: Option<String>,
},
    "Put a track before the track `before`, or last without one.",
    "{name}");

op!(TrackRemove, "timeline track remove", 1, TrackRemoveArgs {
    pub track: String,
},
    "Delete a track and its variable group.",
    "{removed: true, name}");

fn track_of(tx: &Txn, given: &str) -> Result<(String, Track), String> {
    let id = id_of("track", tx.g.timeline(), given)?;
    Ok((id.clone(), tx.g.timeline()[&id].clone()))
}

/// Set or remove the track `id` whole, as one undo step; the graph follows every name that moved.
fn set(tx: &mut Txn, id: &str, track: Option<Track>) -> Result<(), String> {
    tx.key(format!("track {id}"));
    for touched in tx.g.set_track(id, track)? {
        tx.echo(touched);
    }
    Ok(())
}

impl ReadOp for List {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        Ok(json!({ "tracks": tx.g.timeline() }))
    }
}

impl WriteOp for TrackAdd {
    const LABEL: &str = "Add track";
    fn run(tx: &mut Txn, a: TrackAddArgs) -> Result<Value, String> {
        let lanes: Lanes = serde_json::from_value(json!({ "kind": a.kind })).map_err(|_| format!("`kind` is `note` or `curve`, not `{}`", a.kind))?;
        let name = a.name.unwrap_or_else(|| goofi_core::fresh_name("track", 0, |n| tx.g.group_taken(n)));
        let id = tx.g.mint_id("k");
        set(tx, &id, Some(Track { name: name.clone(), lanes, input: String::new() }))?;
        Ok(json!({ "id": id, "name": name }))
    }
}

impl WriteOp for TrackEdit {
    const LABEL: &str = "Edit track";
    fn run(tx: &mut Txn, a: TrackEditArgs) -> Result<Value, String> {
        let (id, mut track) = track_of(tx, &a.track)?;
        track.name = a.to.unwrap_or(track.name);
        track.input = a.input.unwrap_or(track.input);
        match (&mut track.lanes, a.notes, a.range, a.keys, a.rate) {
            (Lanes::Note { notes }, next_notes, None, None, None) => *notes = next_notes.unwrap_or(std::mem::take(notes)),
            (Lanes::Curve { range, rate, keys }, None, next_range, next_keys, next_rate) => {
                *range = next_range.unwrap_or(*range);
                *rate = next_rate.unwrap_or(*rate);
                if let Some(mut next) = next_keys {
                    next.sort_by(|a, b| a.at.total_cmp(&b.at));
                    *keys = next;
                }
            }
            _ => return Err(format!("track `{}`: a note track takes `notes`, a curve track `range`, `keys` and `rate`", track.name)),
        }
        let out = json!({ "name": track.name });
        set(tx, &id, Some(track))?;
        Ok(out)
    }
}

impl WriteOp for TrackMove {
    const LABEL: &str = "Move track";
    fn run(tx: &mut Txn, a: TrackMoveArgs) -> Result<Value, String> {
        let (id, track) = track_of(tx, &a.track)?;
        let before = a.before.map(|b| id_of("track", tx.g.timeline(), &b)).transpose()?;
        tx.g.move_track(&id, before.as_deref());
        Ok(json!({ "name": track.name }))
    }
}

impl WriteOp for TrackRemove {
    const LABEL: &str = "Remove track";
    fn run(tx: &mut Txn, a: TrackRemoveArgs) -> Result<Value, String> {
        let (id, track) = track_of(tx, &a.track)?;
        set(tx, &id, None)?;
        Ok(json!({ "removed": true, "name": track.name }))
    }
}

impl EffectOp for TrackRecord {
    fn run(state: &AppState, a: TrackRecordArgs, caller: &Caller) -> Result<Value, String> {
        let (id, track) = track_of(&Txn::begin(state, caller, false, false), &a.track)?;
        if a.on {
            if track.input.is_empty() {
                return Err(format!("track `{}` has no `input` to record from: `timeline track edit` sets one", track.name));
            }
            state.machines.send(Msg::Record(id));
            return Ok(json!({ "name": track.name, "recording": true }));
        }
        // The thread ends the recording with this instant in it, and answers what it took; one
        // that never ran hangs up.
        let (answer, taken) = std::sync::mpsc::channel();
        state.machines.send(Msg::Recorded(id.clone(), answer));
        if let Ok(take) = taken.recv() {
            // The fit runs with nothing held. Where the track moved meanwhile, it is made once
            // more on the track as it stands, in the write itself.
            let (_, before) = track_of(&Txn::begin(state, caller, false, false), &id)?;
            let mut merged = before.clone();
            merged.merge(&take)?;
            let mut tx = Txn::begin(state, caller, false, true);
            let (_, mut now) = track_of(&tx, &id)?;
            if now != before {
                now.merge(&take)?;
                merged = now;
            }
            tx.label("Record into track".into());
            set(&mut tx, &id, Some(merged))?;
            tx.commit();
        }
        Ok(json!({ "name": track.name, "recording": false }))
    }
}
