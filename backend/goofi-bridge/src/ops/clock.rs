//! The patch clock: play, pause and place the position that every `t` reads.

use serde_json::{json, Value};

use super::{op, EffectOp};
use crate::machines::Msg;
use crate::{AppState, Caller};
use goofi_core::Data;

op!(Set, "clock set", 0, SetArgs {
    pub playing: Option<bool>,
    pub time: Option<f64>,
},
    "Steer the patch clock's position: what `t` is in every expression, and what the machines and the timeline step by. `playing` false pauses it and true resumes it; `time` puts it at that second, so a stop is `{playing: false, time: 0}`, and every machine starts over from there. The engines run on through a pause, and a stamp or a recording never stops: those read the run time. The tempo, the length, and whether the position restarts at the length are the variables `clock.bpm`, `clock.length` and `clock.loop`; `clock.time`, `clock.progress` and `clock.playing` say where it stands. Not undoable, and no change to the patch.",
    "{playing, time}");

impl EffectOp for Set {
    fn run(state: &AppState, a: SetArgs, _: &Caller) -> Result<Value, String> {
        if a.time.is_some_and(|t| !(t.is_finite() && t >= 0.0)) {
            return Err("`time` is seconds of at least 0".into());
        }
        let g = state.graph.lock();
        let time = g.time();
        time.steer(a.playing.unwrap_or(time.playing()), a.time);
        g.variables().publish("clock.playing", Data::number(f64::from(u8::from(time.playing()))));
        // A position put somewhere starts every run over there; a pause or a resume rings the
        // machines' thread through the variable.
        if a.time.is_some() {
            state.machines.send(Msg::Jumped);
        }
        Ok(json!({ "playing": time.playing(), "time": time.position() }))
    }
}
