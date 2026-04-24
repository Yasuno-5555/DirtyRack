pub mod nodes;

use arc_swap::ArcSwap;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use dirtydata_core::ir::Graph;
use dirtydata_core::types::{StableId, NodeKind};
use dirtydata_core::graph_utils;
use crate::nodes::{DspNode, OscillatorNode, GainNode, AddNode, MultiplyNode, NoiseNode, ClipNode, BiquadFilterNode, DelayNode, AssetReaderNode};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A stateful runner for the DSP graph.
struct DspRunner {
    nodes: Vec<(StableId, Box<dyn DspNode>)>,
    node_outputs: HashMap<StableId, [f32; 2]>,
    sorted_ids: Vec<StableId>,
    graph: Graph,
}

impl DspRunner {
    fn new(graph: Graph) -> Self {
        let (sorted_ids, _) = graph_utils::topological_sort(&graph);
        let mut nodes = Vec::new();
        let mut node_outputs = HashMap::new();

        for &id in &sorted_ids {
            if let Some(node) = graph.nodes.get(&id) {
                let name = node.config.get("name")
                    .and_then(|v| v.as_string());
                
                let dsp_node: Box<dyn DspNode> = match name.map(|s| s.as_str()).unwrap_or("Unknown") {
                    "Oscillator" | "Sine" => Box::new(OscillatorNode::new()),
                    "Noise" => Box::new(NoiseNode::new(id.to_string().as_bytes().len() as u64)),
                    "Gain" => Box::new(GainNode),
                    "Add" => Box::new(AddNode),
                    "Multiply" => Box::new(MultiplyNode),
                    "Clip" => Box::new(ClipNode),
                    "Filter" | "Biquad" => Box::new(BiquadFilterNode::new()),
                    "Delay" => Box::new(DelayNode::new(44100 * 2)),
                    "AssetReader" => {
                        let path = node.config.get("path").and_then(|v| v.as_string());
                        if let Some(p) = path {
                            if let Ok(mut reader) = hound::WavReader::open(p) {
                                let spec = reader.spec();
                                let samples: Vec<f32> = match spec.sample_format {
                                    hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
                                    hound::SampleFormat::Int => {
                                        let max = (1u64 << (spec.bits_per_sample - 1)) as f32;
                                        reader.samples::<i32>().map(|s| s.unwrap_or(0) as f32 / max).collect()
                                    }
                                };
                                Box::new(AssetReaderNode::new(Arc::new(samples)))
                            } else {
                                Box::new(GainNode)
                            }
                        } else {
                            Box::new(GainNode)
                        }
                    }
                    _ => Box::new(GainNode),
                };
                nodes.push((id, dsp_node));
                node_outputs.insert(id, [0.0, 0.0]);
            }
        }

        Self {
            nodes,
            node_outputs,
            sorted_ids,
            graph,
        }
    }

    fn process_sample(&mut self, sample_rate: f32) -> [f32; 2] {
        let mut final_out = [0.0, 0.0];

        for (id, dsp_node) in &mut self.nodes {
            let node_ir = &self.graph.nodes[id];
            
            // Gather inputs
            let mut inputs = Vec::new();
            for edge in self.graph.edges.values() {
                if edge.target.node_id == *id {
                    if let Some(out) = self.node_outputs.get(&edge.source.node_id) {
                        inputs.push(out[0]);
                        inputs.push(out[1]);
                    }
                }
            }

            let mut outputs = [0.0, 0.0];
            dsp_node.process(&inputs, &mut outputs, &node_ir.config, sample_rate);
            
            self.node_outputs.insert(*id, outputs);

            if node_ir.kind == NodeKind::Sink {
                final_out[0] += outputs[0];
                final_out[1] += outputs[1];
            }
        }

        final_out
    }
}

pub struct AudioEngine {
    runner_tx: crossbeam_channel::Sender<DspRunner>,
    crash_flag: Arc<AtomicBool>,
    _stream: cpal::Stream,
}

impl AudioEngine {
    pub fn new(initial_graph: Graph) -> Result<Self, Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("No output device available")?;
        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate().0 as f32;
        let channels = config.channels() as usize;

        let (runner_tx, runner_rx) = crossbeam_channel::bounded::<DspRunner>(1);
        let _ = runner_tx.send(DspRunner::new(initial_graph));
        
        let crash_flag = Arc::new(AtomicBool::new(false));

        let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

        let mut current_runner: Option<DspRunner> = None;

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // 1. Check for runner updates
                    if let Ok(new_runner) = runner_rx.try_recv() {
                        current_runner = Some(new_runner);
                    }

                    let Some(runner) = &mut current_runner else {
                        data.fill(0.0);
                        return;
                    };

                    // 2. Sample Domain Processing
                    for frame in data.chunks_mut(channels) {
                        let out = runner.process_sample(sample_rate);
                        for (i, val) in out.iter().enumerate() {
                            if i < frame.len() {
                                frame[i] = *val;
                            }
                        }
                    }
                },
                err_fn,
                None,
            )?,
            _ => return Err("Unsupported sample format".into()),
        };

        stream.play()?;

        Ok(Self {
            runner_tx,
            crash_flag,
            _stream: stream,
        })
    }

    pub fn check_crash(&self) -> bool {
        self.crash_flag.swap(false, Ordering::SeqCst)
    }

    pub fn update_graph(&self, new_graph: Graph) {
        let _ = self.runner_tx.send(DspRunner::new(new_graph));
    }
}

