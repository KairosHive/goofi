---
name: designing-biofeedback
description: Use when a patch turns a brain or body feature into something a person sees or hears — neurofeedback, biofeedback, a sonification or visualisation driven by EEG, heart, breath or any live biosignal. Covers which features to feed back, how to make them prompt but not jittery, and how to design stimulation that matches the target (complexity, a frequency band) without flashing or cheap modulation. Read it also when a feedback patch feels laggy, twitchy, or the person cannot tell what they are controlling.
---

# Designing Biofeedback

## Overview

Biofeedback is a loop: the person does something, the feature moves, the stimulus changes, the
person notices. The loop works only if every link is clear. A feature the person cannot drive,
a lag they cannot connect to their own state, a stimulus whose changes they cannot distinguish
from its own decoration — any one of these breaks the loop, and the patch becomes a screensaver
with electrodes attached.

Build the signal chain as in `patching-biosignals` first; this skill starts at the feature.

## The feature: clear, prompt, steady

**Clear.** One feature, with a known direction, that the person can learn to move. Relative
alpha, a theta/beta ratio, Lempel-Ziv complexity, a FOOOF peak. A sum of many things, or a raw
band power that drifts with impedance, gives the person nothing to hold on to.

**Prompt.** Latency is the buffer length plus whatever smoothing follows, and the person has to
connect a change in the stimulus to what they just did. Use the short end of the buffer range
(2–3 s) and nothing slower in the chain than it needs.

**Steady.** A short buffer makes the feature jitter from chunk to chunk, and a stimulus that
twitches is read as noise, not as feedback. Take the edge off with a light `signal:Smooth` —
a running mean or an exponential decay over about half a second to a second, no more. Smoothing
longer than the buffer only adds lag; it does not add information. Then put the feature on a
known range with `signal:Normalize` over a window of its own past (tens of seconds) so the
stimulus spans its full range for this person, today, and `hold` the statistics once a baseline
is in.

Watch the smoothed feature next to the raw one. If the smoothed one still reacts within a second
of a blink or an eye-close, the latency is right; if it drifts after the person has stopped,
smooth less.

## The stimulus: match the goal

The stimulus should **be** the thing the feature measures, so moving the feature is the same
act as moving the stimulus. This is goal-matching, and it decides what to build:

- **A complexity target** (raise Lempel-Ziv, entropy, dimension): the stimulus grows more
  complex as the feature rises — a visual field that becomes richer, less periodic, more
  detailed; a sound that gains voices, variation, harmonic spread. Falling complexity should
  simplify the stimulus, not just dim it.
- **A frequency target** (more alpha, less beta): the stimulus takes on the character of that
  band — its tempo, its rate of change, its texture — so the environment enhances what the
  brain is asked to do. Do **not** flash or pulse the stimulus at the target frequency to evoke
  it: that drives the band from outside, is unpleasant, and teaches the person nothing.
- **A calm or coherence target**: the stimulus settles, slows, aligns; it does not reward with
  a fanfare.

Stimulus and feature should move in the same direction, so the person's model of the loop is
one sentence long: "when I do this, it does that".

## The stimulus: rich, and quiet about everything else

The stimulus must be worth attending to for minutes. Use the generative graphics
(`graphics:Lenia`, `graphics:Reaction`, `graphics:NeuralCA`, feedback shaders — see
`designing-emergent-shaders`) and the audio plane with real timbre, reverb and voices, not a bar
or a beep. A person will not sit with a bar.

But only **one** thing may move saliently, and it is the feature. Everything else in the stimulus
is either constant, or evolves so slowly and smoothly that it reads as the stimulus's own life,
not as a change. If a clock, an LFO or a random source modulates something the eye or ear
catches, the person cannot tell feedback from decoration, and the loop is gone. Test it: freeze
the feature (hold the normalizer, or feed a constant) and watch. Whatever still visibly moves
is a leak.

Map the feature onto the parameter that changes the stimulus's *nature* (growth rate, number of
voices, harmonic spread, rule parameters), not merely its brightness or volume. Brightness and
volume are the cheapest modulations and the ones that read as a bar in disguise.

## Before handing the patch over

- Walk the chain from feature to stimulus and name the latency at each step. Under 3 s total.
- Freeze the feature; nothing salient moves.
- Drive the feature by hand (a `signal:Variable` in its place) across its range; the stimulus
  goes from one clearly different state to another, both of them good to look at or listen to.
- The direction is right: more of the target makes more of the target's character.
