---
name: patching-biosignals
description: Use when building or reviewing a patch that takes a live or replayed EEG (or other biosignal) stream and derives features from it — band power, spectra, complexity, connectivity. Covers the chain from the stream to the analysis nodes and the mistakes that make features silently wrong: filtering a buffer, buffering too short or too long, analysing raw chunks.
---

# Patching Biosignals

## Overview

A biosignal feature is only as good as the chain in front of it, and most of that chain is the
same in every patch. The order is fixed because each stage has a contract with the next:

```
source  →  EegPreprocess  →  Buffer (2–5 s)  →  analysis
```

The source is `signal:LslIn` for a live device or `eeg:EegPlayback` for a recording. Both emit
**fresh chunks**: `[channels, new samples]`, every sample exactly once, with labels and sample
rate in the meta. Everything downstream relies on that.

## Filter the stream, then buffer the filtered stream

`eeg:EegPreprocess` is a causal filter that keeps state across chunks. It expects each sample
once, so it must sit **directly on the source**. Wire it after a `signal:Buffer` and it sees the
same samples again on every tick — the filter state is corrupted, the output rings, and nothing
reports an error. That is the one wiring mistake this skill exists to prevent.

The buffer comes **after** the filter. The analysis nodes want a window of filtered signal, not
a chunk: a 20 ms chunk has no 1 Hz content and a spectrum of it says nothing. Set the buffer's
size between 2 and 5 seconds:

- below 2 s the spectrum cannot resolve the low bands and every feature jumps with each chunk;
- above 5 s the feature lags the person by seconds, which is fatal for feedback and merely dull
  for analysis.

Pick the short end when the feature drives something in real time and the long end when a steady
estimate matters more than a prompt one. Leave `axis` at time.

## Analysis sits on the buffer

Spectral features go `Buffer → signal:Psd → eeg:EegPowerBands` or `→ eeg:Fooof`. Complexity
and entropy nodes (`complexity:LempelZiv`, `complexity:SampleEntropy`, …) take the buffer
directly. Several analyses may share one buffer; do not add a second buffer per analysis unless
they need different window lengths.

Keep the buffer's shape `[channels, time]`. Reduce over channels (`signal:Reduce`, `signal:Select`)
after the analysis, not before, unless the analysis itself is per-channel and you want fewer of
them: a mean over channels before filtering throws away the reference structure the filter and
the band powers rely on.

## Check the chain before trusting a feature

Read the meta on the buffer's output: it carries the sample rate and the labels from the source.
If either is missing, the preprocess node is not on the raw stream, or the source is not an EEG
stream at all. A feature that does not move when the person blinks, clenches, or closes their eyes
is almost always a wiring fault in this chain, not a tuning problem downstream.

A recording in `eeg:EegPlayback` reproduces every one of these behaviours at the recording's own
rate, so build and verify the chain against playback before a device is on.
