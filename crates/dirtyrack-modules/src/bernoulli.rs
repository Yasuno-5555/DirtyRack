//! Bernoulli Gate — Deterministic Probability Routing
//!
//! # 憲法遵守
//! - トリガー入力時に確率判定を行い、出力AまたはBに信号をルーティング。
//! - 内部の疑似乱数生成器には固定シード（またはボイスシード）を使用し、決定論的再現を保証。

use crate::signal::{
    BuiltinModuleDescriptor, ParamDescriptor, ParamKind, ParamResponse, PortDescriptor,
    PortDirection, RackDspNode, RackProcessContext, SignalType, TriggerDetector,
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

pub struct BernoulliModule {
    rng: ChaCha8Rng,
    trigger: TriggerDetector,
    last_choice: bool, // false = A, true = B
}

impl BernoulliModule {
    pub fn new(_sr: f32) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(0x42), // TODO: Use SeedScope
            trigger: TriggerDetector::new(),
            last_choice: false,
        }
    }
}

impl RackDspNode for BernoulliModule {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [f32],
        params: &[f32],
        _ctx: &RackProcessContext,
    ) {
        let input = inputs[0];
        let trig_in = inputs[1];
        let prob = params[0].clamp(0.0, 1.0);

        if self.trigger.process(trig_in) {
            self.last_choice = self.rng.gen_bool(prob as f64);
        }

        if !self.last_choice {
            outputs[0] = input;
            outputs[1] = 0.0;
        } else {
            outputs[0] = 0.0;
            outputs[1] = input;
        }
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub fn descriptor() -> BuiltinModuleDescriptor {
    BuiltinModuleDescriptor {
        id: "dirty_logic_bernoulli",
        name: "Bernoulli",
        manufacturer: "DirtyRack",
        hp_width: 4,
        visuals: crate::signal::ModuleVisuals::default(),
        tags: &["Builtin"],
        params: &[ParamDescriptor {
            name: "PROB",
            kind: ParamKind::Knob,
            response: ParamResponse::Immediate,
            min: 0.0,
            max: 1.0,
            default: 0.5,
            position: [0.5, 0.4],
            unit: "%",
        }],
        ports: &[
            PortDescriptor {
                name: "IN",
                direction: PortDirection::Input,
                signal_type: SignalType::Audio,
                max_channels: 1,
                position: [0.5, 0.15],
            },
            PortDescriptor {
                name: "TRIG",
                direction: PortDirection::Input,
                signal_type: SignalType::Trigger,
                max_channels: 1,
                position: [0.5, 0.7],
            },
            PortDescriptor {
                name: "OUT A",
                direction: PortDirection::Output,
                signal_type: SignalType::Audio,
                max_channels: 1,
                position: [0.2, 0.9],
            },
            PortDescriptor {
                name: "OUT B",
                direction: PortDirection::Output,
                signal_type: SignalType::Audio,
                max_channels: 1,
                position: [0.8, 0.9],
            },
        ],
        factory: |sr| Box::new(BernoulliModule::new(sr)),
    }
}
