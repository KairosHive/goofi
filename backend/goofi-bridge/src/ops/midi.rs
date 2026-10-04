//! The MIDI devices on the variable bus: list the host's ports, grab one as a group, release it.

use serde_json::{json, Value};

use super::{op, NoArgs, ReadOp, WriteOp};
use crate::Txn;
use goofi_core::variables::{Midi, MIDI_ENTRIES};
use goofi_core::Data;
use goofi_graph::Command;

op!(List, "midi list", 0, NoArgs,
    "Every MIDI input port the host lists, with the group that reads it where one is grabbed, and every grabbed group whose port is gone. Listing also retries the gone ports, so a device plugged back in is read again.",
    "{ports: [{port, group?, channel?, state: present|gone}], reason?} — `reason` names why the host lists nothing");

op!(Grab, "midi grab", 1, GrabArgs {
    pub port: String,
    /// 1 to 16; absent reads every channel.
    pub channel: Option<u8>,
    pub group: Option<String>,
},
    "Grab a MIDI port onto the variable bus as a group — named after the port unless `group` says otherwise — of `cc` (128 controllers in 0..1), `notes` (128 held velocities), `bend` (-1..1) and `pressure` (0..1), written as the device plays. The grab is patch config: a saved patch grabs its devices again on load, and a port the host does not list is held at its last values. The group is config-locked; an expression reads `variables.<group>.cc[74]`.",
    "{group} — the group as stored");

op!(Release, "midi release", 1, ReleaseArgs {
    pub group: String,
},
    "Release a grabbed MIDI device: its port is closed and its group removed with every entry. An expression that read it is told the variable is not defined.",
    "{removed: true}");

impl ReadOp for List {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        let closed = tx.state.midi.resync(&tx.g.variables(), true);
        drop(closed);
        let (ports, reason) = tx.state.midi.list(&tx.g.variables());
        Ok(match reason {
            Some(reason) => json!({ "ports": ports, "reason": reason }),
            None => json!({ "ports": ports }),
        })
    }
}

/// The identifier a port's name reads as: `Launch Control XL:1` is `launch_control_xl_1`.
fn group_of(port: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut base = String::new();
    for c in port.chars() {
        let c = if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' };
        if c != '_' || !base.ends_with('_') {
            base.push(c);
        }
    }
    let base = base.trim_matches('_');
    let base = match base.chars().next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => base.to_string(),
        _ => format!("midi_{base}"),
    };
    if taken(&base) { goofi_core::fresh_name(&base, 2, taken) } else { base }
}

impl WriteOp for Grab {
    fn run(tx: &mut Txn, a: GrabArgs) -> Result<Value, String> {
        if a.channel.is_some_and(|c| !(1..=16).contains(&c)) {
            return Err("`channel` is 1 to 16, or absent for every channel".into());
        }
        let group = match a.group {
            Some(group) if tx.g.group_taken(&group) => return Err(format!("variable group `{group}` already exists")),
            Some(group) => group,
            None => group_of(&a.port, |n| tx.g.group_taken(n)),
        };
        tx.apply(Command::AddVariableGroup { group: group.clone(), at: None })?;
        for (entry, width) in MIDI_ENTRIES {
            let value = Data::numbers(std::iter::repeat_n(0.0, width));
            tx.apply(Command::EditVariable { name: format!("{group}.{entry}"), value: Some(value), at: None, control: None })?;
        }
        tx.apply(Command::MidiVariableGroup { group: group.clone(), midi: Some(Midi { port: a.port, channel: a.channel }) })?;
        Ok(json!({ "group": group }))
    }

    fn label(_: &GrabArgs, o: &Value) -> String {
        format!("Grab MIDI {}", o["group"].as_str().unwrap_or_default())
    }
}

impl WriteOp for Release {
    fn run(tx: &mut Txn, a: ReleaseArgs) -> Result<Value, String> {
        if tx.g.variables().midi(&a.group).is_none() {
            return Err(format!("`{}` reads no MIDI device", a.group));
        }
        tx.apply(Command::MidiVariableGroup { group: a.group.clone(), midi: None })?;
        let entries: Vec<String> = tx.g.variables().entries().map(|(n, _)| n.to_string())
            .filter(|n| goofi_core::variables::split_variable(n).is_some_and(|(g, _)| g == a.group)).collect();
        for name in entries {
            tx.apply(Command::RemoveVariable { name })?;
        }
        tx.apply(Command::RemoveVariableGroup { group: a.group })?;
        Ok(json!({ "removed": true }))
    }

    fn label(a: &ReleaseArgs, _: &Value) -> String {
        format!("Release MIDI {}", a.group)
    }
}
