//! Sampled expressions use the ordinary evaluator at each source-time instant.

use goofi_core::samples::{SampleSpan, CONTROL_DELAY};
use goofi_core::{Data, Value};

use crate::{BindingId, EvalCtx, ExprError, ExprEvaluator, Local};

/// The common interval of sampled inputs. Different clock ties cannot be combined implicitly.
pub fn intersection(locals: &[(String, Local)]) -> Result<Option<SampleSpan>, ExprError> {
    let mut result: Option<SampleSpan> = None;
    for (_, local) in locals {
        let Local::Frame(frame) = local else { continue };
        SampleSpan::validate(frame).map_err(ExprError)?;
        let Some(span) = SampleSpan::of(frame) else { continue };
        let end = span.first.checked_add(span.length as u64).ok_or_else(|| ExprError("sample interval overflows".into()))?;
        result = Some(match result {
            None => span,
            Some(prior) => {
                if prior.clock != span.clock { return Err(ExprError("sampled inputs have different clock ties".into())); }
                let first = prior.first.max(span.first);
                let end = end.min(prior.first + prior.length as u64);
                let length = end.checked_sub(first).filter(|n| *n > 0)
                    .and_then(|n| usize::try_from(n).ok()).ok_or_else(|| ExprError("sampled inputs have no common interval".into()))?;
                SampleSpan { clock: span.clock, first, length }
            }
        });
    }
    Ok(result)
}

/// Evaluate a frame, preserving an input sample grid when one is present.
pub fn eval_frame(evaluator: &dyn ExprEvaluator, id: BindingId, ctx: &EvalCtx<'_>) -> Result<Data, ExprError> {
    match intersection(ctx.locals)? {
        Some(span) => evaluator.eval_sampled(id, ctx, span),
        None => evaluator.eval(id, ctx),
    }
}

/// Check a direct batched call against the interval its inputs actually carry.
pub fn validate(locals: &[(String, Local)], span: SampleSpan) -> Result<(), ExprError> {
    if intersection(locals)? == Some(span) { Ok(()) }
    else { Err(ExprError("sample interval does not match its expression inputs".into())) }
}

/// One sample column as the ordinary numeric vector the expression already knows.
fn column(frame: &Data, sample: u64) -> Result<Data, ExprError> {
    let Some(span) = SampleSpan::of(frame) else { return Ok(frame.clone()) };
    let Value::Array(array) = frame.value() else { return Err(ExprError("sampled input is not numeric".into())) };
    let offset = sample.checked_sub(span.first).and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n < span.length).ok_or_else(|| ExprError("sample is outside its input interval".into()))?;
    Ok(Data::numbers(array.values().skip(offset).step_by(span.length).map(f64::from)))
}

/// Read the sample effective at a logical source instant, without taking a future column.
pub fn at(frame: &Data, logical: goofi_core::time::Tick) -> Result<Option<Data>, ExprError> {
    SampleSpan::validate(frame).map_err(ExprError)?;
    let Some(span) = SampleSpan::of(frame) else { return Ok(Some(frame.clone())) };
    let presentation = logical.checked_add(CONTROL_DELAY).ok_or_else(|| ExprError("sample instant overflows".into()))?;
    if presentation < span.clock.at(span.first) { return Ok(None); }
    let mut sample = span.clock.index(presentation);
    if span.clock.at(sample) > presentation { sample = sample.saturating_sub(1); }
    if sample < span.first || sample >= span.first + span.length as u64 { return Ok(None); }
    column(frame, sample).map(Some)
}

/// Default implementation for evaluators without a batched foreign-language boundary.
pub fn pointwise<E: ExprEvaluator + ?Sized>(evaluator: &E, id: BindingId, ctx: &EvalCtx<'_>, span: SampleSpan) -> Result<Data, ExprError> {
    validate(ctx.locals, span)?;
    let mut shape: Option<Vec<usize>> = None;
    let mut channels: Vec<Vec<f32>> = Vec::new();
    for i in 0..span.length {
        let sample = span.first + i as u64;
        let locals: Vec<_> = ctx.locals.iter().map(|(name, local)| {
            let local = match local { Local::Frame(frame) => Local::Frame(column(frame, sample)?), Local::Value(value) => Local::Value(value.clone()) };
            Ok((name.clone(), local))
        }).collect::<Result<_, ExprError>>()?;
        let at = goofi_core::time::seconds(span.clock.at(sample).saturating_sub(CONTROL_DELAY));
        let value = evaluator.eval(id, &EvalCtx { locals: &locals, t: at, range: ctx.range })?;
        let Value::Array(array) = value.value() else { return Err(ExprError("sampled expression must yield numbers".into())) };
        if let Some(shape) = &shape {
            if shape.as_slice() != array.shape() { return Err(ExprError("sampled expression changes its result shape".into())); }
        } else {
            shape = Some(array.shape().to_vec());
            channels = array.values().map(|_| Vec::with_capacity(span.length)).collect();
            if channels.is_empty() { return Err(ExprError("sampled expression yields no channels".into())); }
        }
        for (channel, value) in channels.iter_mut().zip(array.values()) { channel.push(value); }
    }
    let bytes = channels.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
    Data::array_f32(vec![channels.len(), span.length], bytes, span.meta()).map_err(|error| ExprError(error.to_string()))
}
