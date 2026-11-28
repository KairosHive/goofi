use goofi_sdk::audio::goofi_core::SlotType;
use goofi_sdk::audio::{Face, high, AudioNode, Block, Edge, Manifest, OutputDecl, ParamDecl, ParamSpec, Lanes, Tag, BLOCK};

goofi_sdk::audio::params! {
    GATE = ParamDecl {
        doc: Some("HIGH above zero; an audio reference is one voice per channel"),
        ..ParamDecl::new("env", "gate", ParamSpec::Bool { default: false })
    },
    ATTACK = ParamDecl {
        doc: Some("seconds to full"),
        ..ParamDecl::new("env", "attack", ParamSpec::Num { default: &[0.01], min: 0.0, max: 10.0, int: false, options: &[], face: Face::Plain })
    },
    DECAY = ParamDecl {
        doc: Some("seconds from full to `sustain`"),
        ..ParamDecl::new("env", "decay", ParamSpec::Num { default: &[0.1], min: 0.0, max: 10.0, int: false, options: &[], face: Face::Plain })
    },
    SUSTAIN = ParamDecl {
        doc: Some("the level held while the gate stays HIGH"),
        ..ParamDecl::new("env", "sustain", ParamSpec::Num { default: &[1.0], min: 0.0, max: 1.0, int: false, options: &[], face: Face::Plain })
    },
    RELEASE = ParamDecl {
        doc: Some("seconds from full to silence once the gate drops"),
        ..ParamDecl::new("env", "release", ParamSpec::Num { default: &[0.1], min: 0.0, max: 10.0, int: false, options: &[], face: Face::Plain })
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

goofi_sdk::audio::export!(Env, MANIFEST);
