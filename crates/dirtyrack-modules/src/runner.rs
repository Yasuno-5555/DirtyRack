//! Rack Runner — Audio Thread Execution Engine
//!
//! # 実行モデル: Hybrid Cached Push
//! 1. トポロジカルソート済みのノード配列を走査。
//! 2. `ArcSwap` によるロックフリーなスナップショット差し替え。
//! 3. BTreeMap 排除によるインデックスベースの高速アクセス。

use crate::drift_engine::VoiceDriftEngine;
use crate::signal::{ImperfectionData, RackDspNode, RackProcessContext, SeedScope};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

#[derive(Clone)]
pub struct Connection {
    pub from_module: usize,
    pub from_port: usize,
    pub to_module: usize,
    pub to_port: usize,
}

#[derive(Clone)]
pub struct GraphSnapshot {
    pub order: Vec<usize>,
    pub connections: Vec<Connection>,
    pub port_counts: Vec<(usize, usize)>,
    pub node_ids: Vec<u64>,
}

pub struct RackRunner {
    pub sample_rate: f32,
    pub ctx: RackProcessContext,
    pub active_nodes: Vec<Box<dyn RackDspNode>>,
    pub input_buffers: Vec<Vec<f32>>,
    pub output_buffers: Vec<Vec<f32>>,
    drift_engine: VoiceDriftEngine,
    /// 各ノード・ボイスごとの静的な個体差 [node_idx][voice_idx]
    node_personalities: Vec<[f32; 16]>,
}

impl RackRunner {
    pub fn new(sample_rate: f32, seed_scope: SeedScope) -> Self {
        let seed = match seed_scope {
            SeedScope::Global(s) | SeedScope::Module(s) | SeedScope::Voice(s) => s,
        };
        Self {
            sample_rate,
            ctx: RackProcessContext::new(sample_rate, seed),
            active_nodes: Vec::new(),
            input_buffers: Vec::new(),
            output_buffers: Vec::new(),
            drift_engine: VoiceDriftEngine::new(seed),
            node_personalities: Vec::new(),
        }
    }

    pub fn apply_snapshot(&mut self, snapshot: GraphSnapshot, nodes: Vec<Box<dyn RackDspNode>>) {
        self.active_nodes = nodes;
        self.output_buffers = snapshot
            .port_counts
            .iter()
            .map(|(_, out)| vec![0.0; out * 16])
            .collect();
        self.input_buffers = snapshot
            .port_counts
            .iter()
            .map(|(inp, _)| vec![0.0; inp * 16])
            .collect();

        // 個体差の初期化
        self.node_personalities = Vec::with_capacity(self.active_nodes.len());
        for i in 0..self.active_nodes.len() {
            let node_id = snapshot.node_ids.get(i).copied().unwrap_or(0);
            let mut personalities = [0.0; 16];
            for v in 0..16 {
                // Hash(Project_Seed + Stable_Node_ID + Voice_Index)
                let mut hasher = ChaCha8Rng::seed_from_u64(
                    self.ctx.project_seed ^ (node_id as u64) ^ (v as u64),
                );
                personalities[v] = hasher.gen_range(-1.0..1.0);
            }
            self.node_personalities.push(personalities);
        }
        self.ctx.sample_index = 0;
    }

    pub fn process_sample(&mut self, snapshot: &GraphSnapshot, _params: &[Vec<f32>]) {
        // ドリフトの更新 (aging でスケーリング)
        self.drift_engine.process(&mut self.ctx.imperfection.drift);
        for d in &mut self.ctx.imperfection.drift {
            *d *= 0.1 + self.ctx.aging * 2.0; // 新品でも 0.1、20年物で 2.1 倍の揺れ
        }

        // 1. Clear input buffers
        for buf in &mut self.input_buffers {
            for val in buf {
                *val = 0.0;
            }
        }

        // 2. Map connections
        for conn in &snapshot.connections {
            let src_start = conn.from_port * 16;
            let dst_start = conn.to_port * 16;
            for i in 0..16 {
                self.input_buffers[conn.to_module][dst_start + i] =
                    self.output_buffers[conn.from_module][src_start + i];
            }
        }

        // 3. Process nodes in topological order
        for &idx in &snapshot.order {
            if let Some(node) = self.active_nodes.get_mut(idx) {
                // このノードの個体差を設定
                self.ctx.imperfection.personality = self.node_personalities[idx];

                let ins = &self.input_buffers[idx];
                let outs = &mut self.output_buffers[idx];
                let params: &[f32] = if idx < _params.len() {
                    _params[idx].as_slice()
                } else {
                    &[]
                };

                node.process(ins, outs, params, &self.ctx);

                // Safety: Clamp all outputs to avoid NaN or extreme values causing audio death
                for val in outs {
                    if val.is_nan() {
                        *val = 0.0;
                    } else if *val > 10.0 {
                        *val = 10.0;
                    } else if *val < -10.0 {
                        *val = -10.0;
                    }
                }
            }
        }

        self.ctx.advance();
    }

    pub fn get_output(&self, module_idx: usize, port_idx: usize) -> f32 {
        if let Some(buf) = self.output_buffers.get(module_idx) {
            return *buf.get(port_idx).unwrap_or(&0.0);
        }
        0.0
    }
}
