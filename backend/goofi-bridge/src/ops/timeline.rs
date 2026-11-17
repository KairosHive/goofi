//! The timeline: tracks of notes and of keyframed curves on the patch clock, each a group of
//! variables. A track and an attribute are named by id or by name, as a machine's records are.

use serde_json::{json, Value};

use super::machine::named;
use super::{op, NoArgs, ReadOp, WriteOp};
use crate::Txn;
use goofi_graph::machine::find;
use goofi_graph::timeline::{Lane, Lanes, Note, Track, Unit};

op!(List, "timeline list", 0, NoArgs,
    "Every track of the timeline, under its id, in the order the panel stacks them. A track plays on the patch clock's position (`clock set`): its times are in its `unit`, `seconds` or `beats` of `clock.bpm`. A `note` track holds `notes`, each `{at, length, pitch, velocity}`, and writes `vars.<track>.notes`: the velocity of each held note in 0..1 where a MIDI device's `notes` has its channel 1, so what reads a device reads a track. A `curve` track holds `attributes` by id, each `{name, range: [min, max], keys: [{at, value, curve}]}`, and writes `vars.<track>.<attribute>`: the value eases from each keyframe to the next by that keyframe's `curve` (see `machine attribute add`), and holds before the first and after the last.",
    "{tracks: {id: {name, unit, kind: note, notes} | {name, unit, kind: curve, attributes}}}");

op!(TrackAdd, "timeline track add", 1, TrackAddArgs {
    pub kind: String,
    pub name: Option<String>,
    #[schemars(with = "Option<String>")]
    pub unit: Option<Unit>,
},
    "Add a track of `kind` `note` or `curve` (see `timeline list`). Without a name, use the first free track0/track1/... name; the name is the track's variable group. `unit` is `seconds` (the default) or `beats`.",
    "{id, name} — the track as stored");

op!(TrackEdit, "timeline track edit", 1, TrackEditArgs {
    pub track: String,
    pub to: Option<String>,
    #[schemars(with = "Option<String>")]
    pub unit: Option<Unit>,
    #[schemars(with = "Option<Value>")]
    pub notes: Option<Vec<Note>>,
    pub attributes: Option<serde_json::Map<String, Value>>,
},
    "Change a track. `to` renames it: its variable group moves with what it holds, and every expression that reads it is rewritten. `unit` changes what its times are read in, the numbers as they are. `notes` replaces a note track's notes whole. `attributes` is `{attribute: {name, range, keys} | null}` on a curve track: each replaces the attribute it names whole, by its id or its name — a new name in it renames the attribute — a key that names none adds one, and `null` removes it. Previewable: a drag sends the notes or an attribute as a preview.",
    "{name}");

op!(TrackRemove, "timeline track remove", 1, TrackRemoveArgs {
    pub track: String,
},
    "Delete a track and its variable group. Every timeline panel that hid it forgets it.",
    "{removed: true, name}");

fn track_of(tx: &Txn, given: &str) -> Result<(String, Track), String> {
    let id = find(tx.g.timeline(), given).ok_or_else(|| format!("no track `{given}` — `timeline track add` creates one"))?.to_string();
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
    fn run(tx: &mut Txn, a: TrackAddArgs) -> Result<Value, String> {
        let lanes: Lanes = serde_json::from_value(json!({ "kind": a.kind })).map_err(|_| format!("`kind` is `note` or `curve`, not `{}`", a.kind))?;
        let name = a.name.unwrap_or_else(|| goofi_core::fresh_name("track", 0, |n| tx.g.group_taken(n)));
        let id = tx.g.mint_id("k", |_| false);
        set(tx, &id, Some(Track { name: name.clone(), unit: a.unit.unwrap_or_default(), lanes }))?;
        Ok(json!({ "id": id, "name": name }))
    }

    fn label(_: &TrackAddArgs, o: &Value) -> String {
        format!("Add track {}", named(o))
    }
}

impl WriteOp for TrackEdit {
    fn run(tx: &mut Txn, a: TrackEditArgs) -> Result<Value, String> {
        let (id, mut track) = track_of(tx, &a.track)?;
        track.name = a.to.unwrap_or(track.name);
        track.unit = a.unit.unwrap_or(track.unit);
        match (&mut track.lanes, a.notes, a.attributes) {
            (_, None, None) => {}
            (Lanes::Note { notes }, Some(next), None) => *notes = next,
            (Lanes::Curve { attributes }, None, Some(edits)) => {
                for (given, lane) in edits {
                    let held = find(attributes, &given).map(str::to_string);
                    if lane.is_null() {
                        attributes.shift_remove(&held.ok_or_else(|| format!("no attribute `{given}` in track `{}`", track.name))?);
                        continue;
                    }
                    let mut lane: Lane = serde_json::from_value(lane).map_err(|e| format!("attribute `{given}`: {e}"))?;
                    lane.keys.sort_by(|a, b| a.at.total_cmp(&b.at));
                    attributes.insert(held.unwrap_or_else(|| tx.g.mint_id("a", |id| attributes.contains_key(id))), lane);
                }
            }
            _ => return Err(format!("track `{}`: a note track takes `notes`, a curve track `attributes`", track.name)),
        }
        let out = json!({ "name": track.name });
        set(tx, &id, Some(track))?;
        Ok(out)
    }

    fn label(_: &TrackEditArgs, o: &Value) -> String {
        format!("Edit track {}", named(o))
    }
}

impl WriteOp for TrackRemove {
    fn run(tx: &mut Txn, a: TrackRemoveArgs) -> Result<Value, String> {
        let (id, track) = track_of(tx, &a.track)?;
        set(tx, &id, None)?;
        Ok(json!({ "removed": true, "name": track.name }))
    }

    fn label(_: &TrackRemoveArgs, o: &Value) -> String {
        format!("Remove track {}", named(o))
    }
}
