//! Offline Renderer — Deterministic Batch Execution
//!
//! GUI やオーディオ・コールバックに依存せず、最高速でパッチをレンダリングし、
//! 指定されたサンプル数だけオーディオ・バッファを出力。

use crate::runner::{GraphSnapshot, RackRunner};
use crate::signal::{RackDspNode, SeedScope};

pub struct OfflineRenderer {
    runner: RackRunner,
    snapshot: GraphSnapshot,
}

impl OfflineRenderer {
    pub fn new(
        sample_rate: f32,
        seed_scope: SeedScope,
        snapshot: GraphSnapshot,
        nodes: Vec<Box<dyn RackDspNode>>,
    ) -> Self {
        let mut runner = RackRunner::new(sample_rate, seed_scope);
        runner.apply_snapshot(snapshot.clone(), nodes);
        Self { runner, snapshot }
    }

    /// 指定されたサンプル数だけレンダリングし、ステレオバッファを返す
    /// output_module: 最終出力を取り出すモジュールのインデックス
    pub fn render_block(&mut self, samples: usize, output_module: usize) -> Vec<(f32, f32)> {
        let mut buffer = Vec::with_capacity(samples);
        for _ in 0..samples {
            self.runner.process_sample(&self.snapshot, &[]);
            // L=0, R=1 と仮定（Outputモジュールの仕様に依存）
            let l = self.runner.get_output(output_module, 0);
            let r = self.runner.get_output(output_module, 1);
            buffer.push((l, r));
        }
        buffer
    }
}
