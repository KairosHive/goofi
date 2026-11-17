//! The timeline: tracks of notes and of keyframed curves on the patch clock, each a group of
//! variables. A track is named by id or by name, as a machine's records are.

use serde_json::{json, Value};

use super::machine::id_of;
use super::{op, NoArgs, ReadOp, WriteOp};
use crate::Txn;
use goofi_graph::timeline::{Key, Lanes, Note, Track};

op!(List, "timeline list", 0, NoArgs,
    "Every track of the timeline, under its id, in the order the panel stacks them. A track plays on the patch clock's position (`clock set`): its times are seconds, or beats of `clock.bpm` where `beats` is true. A `note` track holds `notes`, each `{at, length, pitch, velocity}`, and its keyboard as a panel draws it — `range: [low, high]`, the pitches it shows, and `scale` and `root`, whose notes it marks and a gesture lands on, the options of a `Quantize` node — and writes `vars.<track>.notes`: the velocity of each held note in 0..1 where a MIDI device's `notes` has its channel 1, so what reads a device reads a track. A `curve` track holds one curve, `range: [min, max]` and `keys: [{at, value, curve}]`, and writes `vars.<track>.value`: the value eases from each keyframe to the next by that keyframe's `curve` (see `machine attribute add`), and holds before the first and after the last.",
    "{tracks: {id: {name, beats, kind: note, range, scale, root, notes} | {name, beats, kind: curve, range, keys}}}");

op!(TrackAdd, "timeline track add", 1, TrackAddArgs {
    pub kind: String,
    pub name: Option<String>,
    pub beats: Option<bool>,
},
    "Add a track of `kind` `note` or `curve` (see `timeline list`). Without a name, use the first free track0/track1/... name; the name is the track's variable group. Its times are seconds, or beats where `beats` is true.",
    "{id, name} — the track as stored");

op!(TrackEdit, "timeline track edit", 1, TrackEditArgs {
    pub track: String,
    pub to: Option<String>,
    pub beats: Option<bool>,
    #[schemars(with = "Option<Value>")]
    pub notes: Option<Vec<Note>>,
    pub range: Option<[f64; 2]>,
    pub scale: Option<String>,
    pub root: Option<String>,
    #[schemars(with = "Option<Value>")]
    pub keys: Option<Vec<Key>>,
},
    "Change a track. `to` renames it: its variable group moves with what it holds, and every expression that reads it is rewritten. `beats` changes what its times are read in, the numbers as they are. `notes` replaces a note track's notes whole; `range`, `scale` and `root` set its keyboard. `range` and `keys` replace a curve track's range and its keyframes whole. Previewable: a drag sends the notes or the keyframes as a preview.",
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
        let id = tx.g.mint_id("k", |_| false);
        set(tx, &id, Some(Track { name: name.clone(), beats: a.beats.unwrap_or_default(), lanes }))?;
        Ok(json!({ "id": id, "name": name }))
    }
}

impl WriteOp for TrackEdit {
    const LABEL: &str = "Edit track";
    fn run(tx: &mut Txn, a: TrackEditArgs) -> Result<Value, String> {
        let (id, mut track) = track_of(tx, &a.track)?;
        track.name = a.to.unwrap_or(track.name);
        track.beats = a.beats.unwrap_or(track.beats);
        match (&mut track.lanes, a.notes, a.range, a.keys) {
            (Lanes::Note { range, scale, root, notes }, next_notes, next_range, None) => {
                *scale = a.scale.unwrap_or(std::mem::take(scale));
                *root = a.root.unwrap_or(std::mem::take(root));
                if let Some(next) = next_range {
                    let pitch = |p: f64| (p.fract() == 0.0 && (0.0..=127.0).contains(&p)).then_some(p as u8);
                    *range = pitch(next[0]).zip(pitch(next[1])).map(<[u8; 2]>::from).ok_or_else(|| format!("track `{}`: a note track's `range` is two pitches, 0..127", track.name))?;
                }
                *notes = next_notes.unwrap_or(std::mem::take(notes));
            }
            (Lanes::Curve { range, keys }, None, next_range, next_keys) if a.scale.is_none() && a.root.is_none() => {
                *range = next_range.unwrap_or(*range);
                if let Some(mut next) = next_keys {
                    next.sort_by(|a, b| a.at.total_cmp(&b.at));
                    *keys = next;
                }
            }
            _ => return Err(format!("track `{}`: a note track takes `notes`, `scale` and `root`, a curve track `keys`", track.name)),
        }
        let out = json!({ "name": track.name });
        set(tx, &id, Some(track))?;
        Ok(out)
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
