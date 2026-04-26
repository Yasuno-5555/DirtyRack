//! Noise Module — White/Pink Noise
//!
//! # Outputs
//! - WHITE: ホワイトノイズ
//! - PINK: ピンクノイズ (予定)

use crate::signal::{
    ParamDescriptor, PortDescriptor, PortDirection, RackDspNode, RackProcessContext, SeedScope,
    SignalType,
};

pub struct NoiseModule {
    state: u32,
}

impl NoiseModule {
    pub fn new(_sample_rate: f32) -> Self {
        Self { state: 0xACE1 } // Initial seed
    }
}

impl RackDspNode for NoiseModule {
    fn process(
        &mut self,
        _inputs: &[f32],
        outputs: &mut [f32],
        _params: &[f32],
        _ctx: &RackProcessContext,
    ) {
        // Simple Xorshift
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;

        // Map u32 to -5.0..5.0
        let val = (x as f32 / u32::MAX as f32) * 10.0 - 5.0;
        outputs[0] = val;
        outputs[1] = 0.0; // PINK (TODO)
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub fn descriptor() -> crate::signal::BuiltinModuleDescriptor {
    crate::signal::BuiltinModuleDescriptor {
        id: "dirty_noise",
        name: "NOISE",
        manufacturer: "DirtyRack",
        hp_width: 4,
        visuals: crate::signal::ModuleVisuals::default(),
        tags: &["Builtin"],
        params: &[],
        ports: &[
            PortDescriptor {
                name: "WHITE",
                direction: PortDirection::Output,
                signal_type: SignalType::Audio,
                max_channels: 1,
                position: [0.5, 0.7],
            },
            PortDescriptor {
                name: "PINK",
                direction: PortDirection::Output,
                signal_type: SignalType::Audio,
                max_channels: 1,
                position: [0.5, 0.9],
            },
        ],
        factory: |sr| Box::new(NoiseModule::new(sr)),
    }
}
