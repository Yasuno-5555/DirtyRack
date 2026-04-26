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

                    for frame in data.chunks_mut(2) {
                        runner.process_sample(&current_snapshot, &ps);

                        let last_node = current_snapshot.order.last().copied().unwrap_or(0);
                        let out_val = (runner.get_output(last_node, 0) * 0.1).clamp(-1.0, 1.0);
                        for s in frame.iter_mut() {
                            *s = out_val;
                        }
                    }

                    // 鑑識データの収集
                    let mut visual_snapshot = VisualSnapshot::default();
                    for (i, &stable_id) in current_snapshot.node_ids.iter().enumerate() {
                        let mut state = ModuleVisualState::default();
                        if let Some(node) = runner.active_nodes.get(i) {
                            state.forensic = node.get_forensic_data();

                            // ついでにパーソナリティと現在のドリフトを注入
                            if let Some(f) = &mut state.forensic {
                                f.personality_offsets = runner.ctx.imperfection.personality; // TODO: node specific
                                f.current_drift = runner.ctx.imperfection.drift;
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
        let params = vec![vec![0.0; 32]; snapshot.order.len()];
        self.params.store(Arc::new(params));

        let _ = self.topology_tx.send(TopologyUpdate { snapshot, nodes });
    }
}
