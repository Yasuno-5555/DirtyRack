//! Scope Module — Oscilloscope
//!
//! Visual-only module (parasite). No audio outputs.

use crate::signal::{
    ParamDescriptor, ParamKind, ParamResponse, PortDescriptor, PortDirection, RackDspNode,
    RackProcessContext, SeedScope, SignalType,
};

pub struct ScopeModule {
    buffer_ch1: Vec<f32>,
    buffer_ch2: Vec<f32>,
    write_pos: usize,
}

impl ScopeModule {
    pub fn new(_sample_rate: f32) -> Self {
        Self {
            buffer_ch1: vec![0.0; 256],
            buffer_ch2: vec![0.0; 256],
            write_pos: 0,
        }
    }
}

impl RackDspNode for ScopeModule {
    fn process(
        &mut self,
        inputs: &[f32],
        _outputs: &mut [f32],
        _params: &[f32],
        _ctx: &RackProcessContext,
    ) {
        self.buffer_ch1[self.write_pos] = inputs[0];
        self.buffer_ch2[self.write_pos] = inputs[1];
        self.write_pos = (self.write_pos + 1) % 256;
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub fn descriptor() -> crate::signal::BuiltinModuleDescriptor {
    crate::signal::BuiltinModuleDescriptor {
        id: "dirty_scope",
        name: "SCOPE",
        manufacturer: "DirtyRack",
        hp_width: 12,
        visuals: crate::signal::ModuleVisuals::default(),
        tags: &["Builtin"],
        params: &[
            ParamDescriptor {
                name: "TIMEBASE",
                kind: ParamKind::Knob,
                response: ParamResponse::Smoothed { ms: 50.0 },
                min: 0.1,
                max: 10.0,
                default: 1.0,
                position: [0.2, 0.2],
                unit: "",
            },
            ParamDescriptor {
                name: "TRIG",
                kind: ParamKind::Knob,
                response: ParamResponse::Smoothed { ms: 10.0 },
                min: -5.0,
                max: 5.0,
                default: 0.0,
                position: [0.8, 0.2],
                unit: "V",
            },
        ],
        ports: &[
            PortDescriptor {
                name: "CH1",
                direction: PortDirection::Input,
                signal_type: SignalType::Audio,
                max_channels: 1,
                position: [0.2, 0.8],
            },
            PortDescriptor {
                name: "CH2",
                direction: PortDirection::Input,
                signal_type: SignalType::Audio,
                max_channels: 1,
                position: [0.4, 0.8],
            },
            PortDescriptor {
                name: "EXT",
                direction: PortDirection::Input,
                signal_type: SignalType::Trigger,
                max_channels: 1,
                position: [0.8, 0.8],
            },
        ],
        factory: |sr| Box::new(ScopeModule::new(sr)),
    }
}
