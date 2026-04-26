//! Audio Engine — NO-ALLOC / NO-LOCK / STATE-PRESERVING
//!
//! Phase 4: Triple-Buffer による視覚的投影と、
//! crossbeam-channel によるトポロジー更新を実装。

use crate::rack::RackState;
use crate::visual_data::{ModuleVisualState, VisualSnapshot};
use arc_swap::ArcSwap;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{unbounded, Receiver, Sender};
use crossbeam_queue::ArrayQueue;
use dirtyrack_modules::runner::{Connection, GraphSnapshot, RackRunner};
use dirtyrack_modules::{PatchEvent, RackDspNode, SeedScope};
use std::sync::Arc;
use triple_buffer::{Input, Output, TripleBuffer};

pub enum AudioEvent {
    TopologyChanged,
}

pub struct TopologyUpdate {
    pub snapshot: GraphSnapshot,
    pub nodes: Vec<Box<dyn RackDspNode>>,
}

pub struct RackAudioEngine {
    params: Arc<ArcSwap<Vec<Vec<f32>>>>,
    topology_tx: Sender<TopologyUpdate>,
    aging_tx: Sender<f32>,
    _stream: cpal::Stream,
}

impl RackAudioEngine {
    pub fn new(sample_rate: f32) -> Result<(Self, Output<VisualSnapshot>), String> {
        let params = Arc::new(ArcSwap::from_pointee(Vec::new()));
        let (topo_tx, topo_rx) = unbounded::<TopologyUpdate>();
        let (aging_tx, aging_rx) = unbounded::<f32>();

        let (mut visual_in, visual_out) = triple_buffer::triple_buffer(&VisualSnapshot::new());

        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("No output device")?;
        let config = device.default_output_config().map_err(|e| e.to_string())?;

        let mut runner = RackRunner::new(sample_rate, SeedScope::Global(0xDE7E_B11D));
        let mut current_snapshot = GraphSnapshot {
            order: Vec::new(),
            connections: Vec::new(),
            port_counts: Vec::new(),
            node_ids: Vec::new(),
            node_type_ids: Vec::new(),
            modulations: Vec::new(),
            forward_edges: Vec::new(),
            back_edges: Vec::new(),
        };
        let params_inner = Arc::clone(&params);

        let stream = device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // 1. Check for topology updates
                    while let Ok(update) = topo_rx.try_recv() {
                        current_snapshot = update.snapshot;
                        runner.apply_snapshot(current_snapshot.clone(), update.nodes);
                    }

                    // 2. Check for aging updates
                    while let Ok(new_aging) = aging_rx.try_recv() {
                        runner.ctx.aging = new_aging;
                    }

                    let ps = params_inner.load();

                    // Find Audio Out module index
                    let output_node_idx = current_snapshot
                        .node_type_ids
                        .iter()
                        .position(|id| id == "dirty_output");

                    for frame in data.chunks_mut(2) {
                        runner.process_sample(&current_snapshot, &ps);

                        let (mut left, mut right) = (0.0, 0.0);
                        if let Some(idx) = output_node_idx {
                            // Audio Out module exists - read its inputs (which were pushed as outputs by zero-latency logic)
                            // or better, we can have OutputModule store them in its own output buffer.
                            left = runner.get_output(idx, 0);
                            right = runner.get_output(idx, 1);
                        } else {
                            // Fallback: use the last node's output if no Audio Out is present
                            let last_node = current_snapshot.order.last().copied().unwrap_or(0);
                            left = runner.get_output(last_node, 0);
                            right = left;
                        }

                        let master_gain = 0.3; // Default master gain
                        frame[0] = (left * master_gain).clamp(-1.0, 1.0);
                        frame[1] = (right * master_gain).clamp(-1.0, 1.0);
                    }

                    // 鑑識データの収集
                    let mut visual_snapshot = VisualSnapshot::default();
                    for (i, &stable_id) in current_snapshot.node_ids.iter().enumerate() {
                        let mut state = ModuleVisualState::default();
                        if let Some(node) = runner.active_nodes.get(i) {
                            state.forensic = node.get_forensic_data();

                            // ついでにパーソナリティと現在のドリフト、エンジン統計を注入
                            if let Some(f) = &mut state.forensic {
                                f.personality_offsets = runner.node_personalities[i]; 
                                f.current_drift = runner.drift_engine.current_drift();
                                f.stats = runner.stats[i];
                            }
                        }

                        for p_idx in 0..current_snapshot.port_counts[i].1 {
                            state.outputs.push(runner.get_output(i, p_idx));
                        }
                        visual_snapshot.modules.insert(stable_id, state);
                    }

                    visual_in.write(visual_snapshot);
                },
                |err| eprintln!("Audio error: {}", err),
                None,
            )
            .map_err(|e| e.to_string())?;

        stream.play().map_err(|e| e.to_string())?;

        let engine = Self {
            params,
            topology_tx: topo_tx,
            aging_tx,
            _stream: stream,
        };

        Ok((engine, visual_out))
    }

    pub fn sync_aging(&self, aging: f32) -> Result<(), String> {
        self.aging_tx.send(aging).map_err(|e| e.to_string())
    }

    pub fn update_topology(&self, snapshot: GraphSnapshot, nodes: Vec<Box<dyn RackDspNode>>) {
        // Wait-free Topology Update: We send the new nodes and snapshot to the audio thread
        // where it will be swapped safely.
        let _ = self.topology_tx.send(TopologyUpdate { snapshot, nodes });
    }
}
