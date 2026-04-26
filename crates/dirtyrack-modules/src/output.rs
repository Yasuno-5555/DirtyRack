//! Output Module — ラックの最終出口。
//! DirtyRack (Mono) から DirtyData (Stereo) へのブリッジを担う。

use crate::signal::{
    ParamDescriptor, ParamKind, ParamResponse, PortDescriptor, PortDirection, RackDspNode,
    RackProcessContext, SeedScope, SignalType,
};

pub struct OutputModule {}

impl OutputModule {
    pub fn new(_sample_rate: f32) -> Self {
        Self {}
    }
}

impl RackDspNode for OutputModule {
    fn process(
        &mut self,
        inputs: &[f32],
        _outputs: &mut [f32],
        params: &[f32],
        _ctx: &RackProcessContext,
    ) {
        let _in_left = inputs[0];
        let _in_right = inputs[1];
        let _master = params[0];

        // 実際にはオーディオスレッドの最終バッファに書き込む
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub fn descriptor() -> crate::signal::BuiltinModuleDescriptor {
    crate::signal::BuiltinModuleDescriptor {
        id: "dirty_output",
        name: "AUDIO OUT",
        manufacturer: "DirtyRack",
        hp_width: 6,
        visuals: crate::signal::ModuleVisuals::default(),
        tags: &["Builtin"],
        params: &[ParamDescriptor {
            name: "MASTER",
            kind: ParamKind::Knob,
            response: ParamResponse::Smoothed { ms: 20.0 },
            min: 0.0,
            max: 1.0,
            default: 0.7,
            position: [0.5, 0.4],
            unit: "dB",
        }],
        ports: &[
            PortDescriptor {
                name: "LEFT",
                direction: PortDirection::Input,
                signal_type: SignalType::Audio,
                max_channels: 1,
                position: [0.3, 0.8],
            },
            PortDescriptor {
                name: "RIGHT",
                direction: PortDirection::Input,
                signal_type: SignalType::Audio,
                max_channels: 1,
                position: [0.7, 0.8],
            },
        ],
        factory: |sr| Box::new(OutputModule::new(sr)),
    }
}
