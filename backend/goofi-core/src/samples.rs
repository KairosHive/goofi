//! A sampled frame's exact position on the patch clock.

use std::collections::BTreeMap;

use crate::time::{Tick, TICKS_PER_SECOND};
use crate::{Data, Meta, MetaValue, Value};

const KEY: &str = "samples";
/// Control sources finish an instant before its audio samples are due.
pub const CONTROL_DELAY: Tick = TICKS_PER_SECOND / 100;
pub const HISTORY: Tick = TICKS_PER_SECOND / 4;

/// The audio clock's current tie, read outside the callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleClock {
    pub patch_epoch: u64,
    pub epoch: u64,
    pub origin: Tick,
    pub first: u64,
    pub rate: u32,
}

impl SampleClock {
    pub fn at(&self, sample: u64) -> Tick {
        self.origin.saturating_add(Tick::from(sample.saturating_sub(self.first)) * TICKS_PER_SECOND / Tick::from(self.rate.max(1)))
    }

    /// First sample at or after this instant; never round a sub-sample edge down.
    pub fn index(&self, at: Tick) -> u64 {
        let offset = at.saturating_sub(self.origin);
        let rate = Tick::from(self.rate);
        let index = offset / TICKS_PER_SECOND * rate + (offset % TICKS_PER_SECOND * rate).div_ceil(TICKS_PER_SECOND);
        self.first.saturating_add(index.min(u64::MAX as Tick) as u64)
    }
}

/// An array `[channels, length]` whose sample numbers share one clock tie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleSpan {
    pub clock: SampleClock,
    pub first: u64,
    pub length: usize,
}

impl SampleSpan {
    pub fn remove(meta: &mut Meta) { meta.set(KEY, MetaValue::Null); }
    pub fn meta(&self) -> Meta {
        Meta::new().with_sfreq(Some(f64::from(self.clock.rate))).with(KEY, MetaValue::Map(BTreeMap::from([
            ("patch_epoch".into(), MetaValue::Uint(self.clock.patch_epoch)),
            ("epoch".into(), MetaValue::Uint(self.clock.epoch)),
            ("origin_hi".into(), MetaValue::Uint((self.clock.origin >> 64) as u64)),
            ("origin_lo".into(), MetaValue::Uint(self.clock.origin as u64)),
            ("anchor".into(), MetaValue::Uint(self.clock.first)),
            ("first".into(), MetaValue::Uint(self.first)),
        ])))
    }

    pub fn of(data: &Data) -> Option<Self> {
        let Value::Array(array) = data.value() else { return None };
        Self::of_shape(data.meta(), array.shape())
    }

    fn of_shape(meta: &Meta, shape: &[usize]) -> Option<Self> {
        let MetaValue::Map(map) = meta.get(KEY)? else { return None };
        let get = |name: &str| match map.get(name) {
            Some(MetaValue::Uint(n)) => Some(*n),
            Some(MetaValue::Int(n)) if *n >= 0 => Some(*n as u64),
            _ => None,
        };
        let rate = meta.sfreq()?;
        if rate <= 0.0 || rate > f64::from(u32::MAX) || rate.fract() != 0.0 { return None; }
        let rate = rate as u32;
        let [channels, length] = shape else { return None };
        if *channels == 0 || *length == 0 { return None; }
        let span = Self {
            clock: SampleClock { patch_epoch: get("patch_epoch")?, epoch: get("epoch")?,
                origin: Tick::from(get("origin_hi")?) << 64 | Tick::from(get("origin_lo")?), first: get("anchor")?, rate },
            first: get("first")?, length: *length,
        };
        let end = span.first.checked_add(span.length as u64)?;
        if span.first < span.clock.first { return None; }
        span.clock.origin.checked_add(Tick::from(end - span.clock.first) * TICKS_PER_SECOND / Tick::from(rate))?;
        Some(span)
    }

    /// A malformed reserved sample position must not become an unclocked frame.
    pub fn validate(data: &Data) -> Result<(), String> {
        let shape = match data.value() { Value::Array(array) => array.shape(), _ => &[] };
        Self::validate_shape(data.meta(), shape)
    }

    /// Viewer encoders use the same clock validation without changing their sample type.
    pub fn validate_shape(meta: &Meta, shape: &[usize]) -> Result<(), String> {
        if meta.get(KEY).is_some() && Self::of_shape(meta, shape).is_none() {
            return Err("invalid sampled frame: check its clock, interval and [channels, samples] shape".into());
        }
        Ok(())
    }
}

impl Data {
    pub fn with_sample_span(&self, span: SampleSpan) -> Data {
        let mut meta = self.meta().clone();
        meta.set_sfreq(Some(f64::from(span.clock.rate)));
        if let Some(value) = span.meta().get(KEY) { meta.set(KEY, value.clone()); }
        Data(std::sync::Arc::new(crate::DataInner { value: self.value().clone(), meta }))
    }
    /// The current control value of a sampled frame; the samples stay on its producer wire.
    pub fn control_value(&self) -> Data {
        if SampleSpan::of(self).is_none() { return self.clone(); }
        Data::numbers(crate::control::numbers(self))
    }
}
