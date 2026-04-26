//! Biquad Filter (VCF-2) — Precision State Variable Filter
//!
//! # 憲法遵守
//! - `libm` による数学的同一性。
//! - LP/HP/BP/Notch のマルチモード。
//! - Moog よりもレゾナンスの効きがシャープで数学的に正確。

use crate::signal::{
    BuiltinModuleDescriptor, ParamDescriptor, ParamKind, ParamResponse, PortDescriptor,
    PortDirection, RackDspNode, RackProcessContext, SeedScope, SignalType,
};

pub struct BiquadModule {
    sample_rate: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl BiquadModule {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }
}

impl RackDspNode for BiquadModule {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [f32],
        params: &[f32],
        _ctx: &RackProcessContext,
    ) {
        let input = inputs[0];
        let cutoff = params[0].max(0.01).min(10.0); // 0..10V -> Hz
        let res = params[1].max(0.01).min(10.0);
        let mode = params[2] as usize;

        // Hz conversion: 10V = 20kHz, 0V = 20Hz (exponential)
        let freq = 20.0 * libm::powf(1000.0, cutoff / 10.0);
        let q = 0.5 + res * 2.0;

        let omega = 2.0 * std::f32::consts::PI * freq / self.sample_rate;
        let sin_w = libm::sinf(omega);
        let cos_w = libm::cosf(omega);
        let alpha = sin_w / (2.0 * q);

        let (b0, b1, b2, a0, a1, a2) = match mode {
            0 => {
                // LP
                let b1 = 1.0 - cos_w;
                let b0 = b1 * 0.5;
                (b0, b1, b0, 1.0 + alpha, -2.0 * cos_w, 1.0 - alpha)
            }
            1 => {
                // HP
                let b1 = -(1.0 + cos_w);
                let b0 = -b1 * 0.5;
                (b0, b1, b0, 1.0 + alpha, -2.0 * cos_w, 1.0 - alpha)
            }
            2 => {
                // BP
                (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos_w, 1.0 - alpha)
            }
            _ => {
                // Notch
                (
                    1.0,
                    -2.0 * cos_w,
                    1.0,
                    1.0 + alpha,
                    -2.0 * cos_w,
                    1.0 - alpha,
                )
            }
        };

        let out = (b0 / a0) * input + (b1 / a0) * self.x1 + (b2 / a0) * self.x2
            - (a1 / a0) * self.y1
            - (a2 / a0) * self.y2;

        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = out;

        outputs[0] = out;
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub fn descriptor() -> BuiltinModuleDescriptor {
    BuiltinModuleDescriptor {
        id: "dirty_vcf_biquad",
        name: "VCF-2",
        manufacturer: "DirtyRack",
        hp_width: 6,
        visuals: crate::signal::ModuleVisuals::default(),
        tags: &["Builtin"],
        params: &[
            ParamDescriptor {
                name: "CUTOFF",
                kind: ParamKind::Knob,
                response: ParamResponse::Smoothed { ms: 10.0 },
                min: 0.0,
                max: 10.0,
                default: 5.0,
                position: [0.5, 0.2],
                unit: "V",
            },
            ParamDescriptor {
                name: "RES",
                kind: ParamKind::Knob,
                response: ParamResponse::Smoothed { ms: 10.0 },
                min: 0.0,
                max: 10.0,
                default: 0.5,
                position: [0.5, 0.4],
                unit: "Q",
            },
            ParamDescriptor {
                name: "MODE",
                kind: ParamKind::Switch { positions: 4 },
                response: ParamResponse::Immediate,
                min: 0.0,
                max: 3.0,
                default: 0.0,
                position: [0.5, 0.7],
                unit: "",
            },
        ],
        ports: &[
            PortDescriptor {
                name: "IN",
                direction: PortDirection::Input,
                signal_type: SignalType::Audio,
                max_channels: 1,
                position: [0.2, 0.9],
            },
            PortDescriptor {
                name: "OUT",
                direction: PortDirection::Output,
                signal_type: SignalType::Audio,
                max_channels: 1,
                position: [0.8, 0.9],
            },
        ],
        factory: |sr| Box::new(BiquadModule::new(sr)),
    }
}
