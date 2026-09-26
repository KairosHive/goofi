use goofi_audio_sdk::goofi_core::SlotType;
use goofi_audio_sdk::{high, AudioNode, Block, Edge, Manifest, OutputDecl, ParamDecl, ParamSpec, Lanes, Tag, BLOCK};

goofi_audio_sdk::params! {
    GATE = ParamDecl {
        group: "env",
        name: "gate",
        spec: ParamSpec::Bool { default: false },
        expression: None,
        doc: Some("HIGH above zero; an audio reference is one voice per channel"),
        section: 0,
        show: None,
    },
    ATTACK = ParamDecl {
        group: "env",
        name: "attack",
        spec: ParamSpec::Float { default: 0.01, min: 0.0, max: 10.0 },
        expression: None,
        doc: Some("seconds to full"),
        section: 0,
        show: None,
    },
    DECAY = ParamDecl {
        group: "env",
        name: "decay",
        spec: ParamSpec::Float { default: 0.1, min: 0.0, max: 10.0 },
        expression: None,
        doc: Some("seconds from full to `sustain`"),
        section: 0,
        show: None,
    },
    SUSTAIN = ParamDecl {
        group: "env",
        name: "sustain",
        spec: ParamSpec::Float { default: 1.0, min: 0.0, max: 1.0 },
        expression: None,
        doc: Some("the level held while the gate stays HIGH"),
        section: 0,
        show: None,
    },
    RELEASE = ParamDecl {
        group: "env",
        name: "release",
        spec: ParamSpec::Float { default: 0.1, min: 0.0, max: 10.0 },
        expression: None,
        doc: Some("seconds from full to silence once the gate drops"),
        section: 0,
        show: None,
    },
}

static OUTS: &[OutputDecl] = &[OutputDecl { name: "out", kind: SlotType::Audio }];

static MANIFEST: Manifest = Manifest {
    tags: &[Tag::Transform],
    doc: "An ADSR in [0, 1] over `gate`, one voice per channel of it.",
    inputs: &[],
    outputs: OUTS,
    params: PARAMS,
};

#[derive(Clone, Copy, Default, PartialEq)]
enum Stage {
    #[default]
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Default)]
struct Env {
    stage: Lanes<Stage>,
    level: Lanes<f32>,
    edge: Lanes<Edge>,
    step: f32,
}

impl AudioNode for Env {
    fn prepare(&mut self, rate: f64) {
        self.step = 1.0 / rate as f32;
    }

    fn process(&mut self, b: &mut Block<'_>) {
        let out = &mut b.outs[0];
        let width = out.channels() as usize;
        let (stages, levels, edges) = (self.stage.fit(width), self.level.fit(width), self.edge.fit(width));
        let step = self.step;
        for c in 0..width {
            let gate = b.params[P::GATE].chan(c);
            let attack = b.params[P::ATTACK].chan(c);
            let decay = b.params[P::DECAY].chan(c);
            let sustain = b.params[P::SUSTAIN].chan(c);
            let release = b.params[P::RELEASE].chan(c);
            let (stage, level, edge) = (&mut stages[c], &mut levels[c], &mut edges[c]);
            let samples = out.chan_mut(c);
            for i in 0..BLOCK {
                if edge.rising(gate[i]) {
                    *stage = Stage::Attack;
                } else if !high(gate[i]) && !matches!(*stage, Stage::Idle | Stage::Release) {
                    *stage = Stage::Release;
                }
                let per = |seconds: f32| step / seconds.max(1e-4);
                match *stage {
                    Stage::Attack => {
                        *level += per(attack[i]);
                        if *level >= 1.0 {
                            *level = 1.0;
                            *stage = Stage::Decay;
                        }
                    }
                    Stage::Decay => {
                        *level -= per(decay[i]) * (1.0 - sustain[i]);
                        if *level <= sustain[i] {
                            *level = sustain[i];
                            *stage = Stage::Sustain;
                        }
                    }
                    Stage::Sustain => *level = sustain[i],
                    Stage::Release => {
                        *level -= per(release[i]);
                        if *level <= 0.0 {
                            *level = 0.0;
                            *stage = Stage::Idle;
                        }
                    }
                    Stage::Idle => *level = 0.0,
                }
                samples[i] = *level;
            }
        }
    }
}

goofi_audio_sdk::export!(Env, MANIFEST);
