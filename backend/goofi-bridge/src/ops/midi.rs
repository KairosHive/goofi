//! The MIDI devices on the variable bus: list the host's ports, and learn from every one of them.

use serde_json::{json, Value};

use super::{op, EffectOp, NoArgs, ReadOp};
use crate::{AppState, Caller, Txn};

op!(List, "midi list", 0, NoArgs,
    "Every MIDI input port the host lists, with the group it is read as — named after the port — and whether it is open. A port is open while the patch reads its group, in an expression, a Variable node or a followed variable, and whole for the length of a learn.",
    "{ports: [{port, group, open}], learning, reason?} — `reason` names why the host lists nothing");

op!(Learn, "midi learn", 1, LearnArgs {
    /// `true` starts a learn, `false` ends it; the learn ends by itself after thirty seconds.
    pub on: bool,
},
    "Open every MIDI port the host lists that is not held elsewhere, for thirty seconds: each is a group of `cc` and `notes` (16 channels of 128, at `(channel - 1) * 128 + number`), `bend` and `pressure` (one a channel), written as the device plays. A reader of the group — an expression, a Variable node, a followed variable — keeps its device open after the learn; every other port is let go when the learn ends.",
    "{groups: [{group, port}], seconds} — the groups open now, and how long the learn lasts");

impl ReadOp for List {
    fn run(tx: &mut Txn, _: NoArgs) -> Result<Value, String> {
        let reason = tx.state.midi.probe();
        let (ports, learning) = tx.state.midi.list();
        Ok(match reason {
            Some(reason) => json!({ "ports": ports, "learning": learning, "reason": reason }),
            None => json!({ "ports": ports, "learning": learning }),
        })
    }
}

impl EffectOp for Learn {
    fn run(state: &AppState, a: LearnArgs, _: &Caller) -> Result<Value, String> {
        state.midi.learn(a.on);
        if a.on {
            state.midi.probe();
        }
        crate::resync_and_broadcast(state);
        let groups: Vec<Value> = state.midi.open().into_iter().map(|(group, port)| json!({ "group": group, "port": port })).collect();
        Ok(json!({ "groups": groups, "seconds": crate::midi::LEARN.as_secs() }))
    }
}
