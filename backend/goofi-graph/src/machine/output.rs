//! Immutable output segments from settled machine state. This is a data-plane projection.

use std::collections::VecDeque;

use goofi_core::ease::Curve;
use goofi_core::samples::{SampleClock, SampleSpan, CONTROL_DELAY, HISTORY};
use goofi_core::time::Tick;
use goofi_core::{Data, Value};
use indexmap::IndexMap;

use super::{Head, Machines};

#[derive(Clone, PartialEq)]
struct ValueSpan {
    from: Data,
    travel: Option<(Tick, Tick, Curve, Data)>,
}

#[derive(Default)]
pub(super) struct Outputs {
    values: IndexMap<(u64, u64), VecDeque<(Tick, ValueSpan)>>,
}

impl Outputs {
    fn record(&mut self, key: (u64, u64), at: Tick, span: ValueSpan) {
        let history = self.values.entry(key).or_default();
        if history.back().is_none_or(|(_, last)| *last != span) {
            if history.back().is_some_and(|(last, _)| *last == at) { history.pop_back(); }
            history.push_back((at, span));
        }
        while history.len() > 1 && history.get(1).is_some_and(|(next, _)| *next < at.saturating_sub(HISTORY)) {
            history.pop_front();
        }
    }

    fn render(&self, key: (u64, u64), clock: SampleClock, next: u64, cutoff: Tick) -> Option<Data> {
        let history = self.values.get(&key)?;
        let start = history.front()?.0.max(cutoff.saturating_sub(HISTORY));
        let first = clock.index(start + CONTROL_DELAY).max(next).max(clock.first);
        let end = clock.index(cutoff + CONTROL_DELAY);
        let length = usize::try_from(end.checked_sub(first)?).ok().filter(|n| *n > 0)?;
        let Value::Array(latest) = history.back()?.1.from.value() else { return None };
        let channels = latest.shape().iter().product();
        if channels == 0 { return None; }
        let mut samples = vec![0.0f32; channels * length];
        let converted: Vec<_> = history.iter().map(|(at, value)| {
            let from = match value.from.value() { Value::Array(array) => array.values().collect::<Vec<_>>(), _ => Vec::new() };
            let target = value.travel.as_ref().and_then(|(_, _, _, target)| match target.value() { Value::Array(array) => Some(array.values().collect::<Vec<_>>()), _ => None });
            (*at, value, from, target)
        }).collect();
        let mut versions = converted.iter().peekable();
        let mut version = versions.next()?;
        for i in 0..length {
            let at = clock.at(first + i as u64).saturating_sub(CONTROL_DELAY);
            while versions.peek().is_some_and(|(next, _, _, _)| *next <= at) { version = versions.next()?; }
            let value = &version.1;
            let from = &version.2;
            let (blend, target) = match &value.travel {
                Some((start, end, curve, _)) => {
                    let t = curve.at(at.saturating_sub(*start) as f64 / (end - start) as f64);
                    (t, version.3.as_ref()?)
                }
                None => (0.0, from),
            };
            for c in 0..channels {
                let a = *from.get(c).or_else(|| from.last())?;
                let b = *target.get(c).or_else(|| target.last())?;
                samples[c * length + i] = goofi_core::control::mix(a, b, blend);
            }
        }
        Data::array_f32(vec![channels, length], samples.iter().flat_map(|v| v.to_le_bytes()).collect(), SampleSpan { clock, first, length }.meta()).ok()
    }
}

fn attribute(head: &Head, name: &str) -> Option<ValueSpan> {
    let from = head.held.get(name)?.clone();
    if !matches!(from.value(), Value::Array(_)) { return None; }
    let travel = head.flight.as_ref().and_then(|flight| flight.target.get(name).filter(|target| match (from.value(), target.value()) {
        (Value::Array(a), Value::Array(b)) => a.shape() == b.shape(),
        _ => false,
    }).map(|target| (flight.start, flight.end, flight.curve, target.clone())));
    Some(ValueSpan { from, travel })
}

impl Machines {
    pub(super) fn record_output(&mut self, at: Tick) {
        let mut keys = Vec::new();
        for (name, run) in &self.runs {
            let machine = &self.config[name];
            for (ph, head) in &run.heads {
                let owner = machine.playheads[ph].identity.generation();
                for (name, attr) in &machine.attributes {
                    let key = (owner, attr.identity.generation());
                    if let Some(value) = attribute(head, name) { keys.push(key); self.outputs.record(key, at, value); }
                }
            }
        }
        self.outputs.values.retain(|key, _| keys.contains(key));
    }

    /// Numeric samples of completed logical time, on the audio engine's exact grid.
    pub fn sampled_writes(&self, clock: SampleClock, next: u64, cutoff: Tick) -> Vec<(String, Data)> {
        self.config.values().flat_map(|machine| machine.playheads.iter().flat_map(|(ph, head)| {
            machine.attributes.iter().filter_map(move |(a, attr)| self.outputs.render((head.identity.generation(), attr.identity.generation()), clock, next, cutoff).map(|data| (format!("{ph}.{a}"), data)))
        })).collect()
    }
}
