pub mod nodes;

use arc_swap::ArcSwap;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use dirtydata_core::ir::Graph;
use dirtydata_core::types::{StableId, NodeKind};
use dirtydata_core::graph_utils;
use crate::nodes::{DspNode, OscillatorNode, GainNode, AddNode, MultiplyNode, NoiseNode, ClipNode, BiquadFilterNode, DelayNode, AssetReaderNode, TriggerNode, EnvelopeNode, AutomationNode, ProcessContext, MidiInNode, MidiEvent};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct ParameterUpdate {
    pub node_id: StableId,
    pub param: String,
    pub value: f32,
}

/// A stateful runner for the DSP graph.
struct DspRunner {
    nodes: Vec<(StableId, Box<dyn DspNode>)>,
    // node_id -> [port_index -> [L, R]]
    node_outputs: HashMap<StableId, Vec<[f32; 2]>>,
    graph: Graph,
}

impl DspRunner {
    fn new(graph: Graph, midi_rx: Option<crossbeam_channel::Receiver<MidiEvent>>) -> Self {
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
                    "Gain" => Box::new(GainNode::new()),
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
                            } else { Box::new(GainNode::new()) }
                        } else { Box::new(GainNode::new()) }
                    }
                    "Trigger" => Box::new(TriggerNode),
                    "Envelope" | "ADSR" => Box::new(EnvelopeNode::new()),
                    "Automation" => Box::new(AutomationNode),
                    "MidiIn" => {
                        if let Some(rx) = &midi_rx {
                            Box::new(MidiInNode::new(rx.clone()))
                        } else {
                            Box::new(GainNode::new())
                        }
                    }
                    _ => Box::new(GainNode::new()),
                };
                nodes.push((id, dsp_node));
                
                // Pre-allocate 4 stereo ports per node for simplicity, or 1 if standard
                node_outputs.insert(id, vec![[0.0, 0.0]; 4]);
            }
        }

        Self { nodes, node_outputs, graph }
    }

    fn process_sample(&mut self, ctx: &ProcessContext) -> [f32; 2] {
        let mut final_out = [0.0, 0.0];

        for (id, dsp_node) in &mut self.nodes {
            let node_ir = &self.graph.nodes[id];
            
            // Gather inputs
            let mut inputs = Vec::new();
            for edge in self.graph.edges.values() {
                if edge.target.node_id == *id {
                    if let Some(ports) = self.node_outputs.get(&edge.source.node_id) {
                        let port_idx = match edge.source.port_name.as_str() {
                            "out" | "gate" | "0" => 0,
                            "pitch" | "1" => 1,
                            "velocity" | "2" => 2,
                            _ => 0,
                        };
                        if let Some(out) = ports.get(port_idx) {
                            inputs.push(out[0]);
                            inputs.push(out[1]);
                        }
                    }
                }
            }

            let mut outputs = vec![[0.0, 0.0]; 4]; // Work buffer
            dsp_node.process(&inputs, &mut outputs, &node_ir.config, ctx);
            
            self.node_outputs.insert(*id, outputs);

            if node_ir.kind == NodeKind::Sink {
                if let Some(ports) = self.node_outputs.get(id) {
                    final_out[0] += ports[0][0];
                    final_out[1] += ports[0][1];
                }
            }
        }

        final_out
    }

    pub fn update_parameter(&mut self, node_id: StableId, param: &str, value: f32) {
        for (id, node) in &mut self.nodes {
            if *id == node_id {
                node.update_parameter(param, value);
                break;
            }
        }
    }
}

pub struct AudioEngine {
    runner_tx: crossbeam_channel::Sender<DspRunner>,
    param_tx: crossbeam_channel::Sender<ParameterUpdate>,
    midi_rx: crossbeam_channel::Receiver<MidiEvent>,
    crash_flag: Arc<AtomicBool>,
    _midi_conn: Option<midir::MidiInputConnection<()>>,
    _stream: cpal::Stream,
}

impl AudioEngine {
    pub fn new(initial_graph: Graph) -> Result<Self, Box<dyn std::error::Error>> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("No output device available")?;
        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate().0 as f32;
        let channels = config.channels() as usize;

        let (midi_tx, midi_rx) = crossbeam_channel::unbounded::<MidiEvent>();
        let (param_tx, param_rx) = crossbeam_channel::unbounded::<ParameterUpdate>();
        
        let global_sample_index_atomic = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let gsi_for_midi = global_sample_index_atomic.clone();

        let crash_flag = Arc::new(AtomicBool::new(false));

        // MIDI Input setup
        let midi_in = midir::MidiInput::new("DirtyData MIDI Input")?;
        let ports = midi_in.ports();
        let _midi_conn = if let Some(port) = ports.first() {
            let conn = midi_in.connect(port, "dirtydata-midi-port", move |_stamp, message, _| {
                if message.len() >= 3 {
                    let event = MidiEvent {
                        sample_index: gsi_for_midi.load(Ordering::Relaxed),
                        message: [message[0], message[1], message[2]],
                    };
                    let _ = midi_tx.send(event);
                }
            }, ())?;
            Some(conn)
        } else {
            None
        };

        let (runner_tx, runner_rx) = crossbeam_channel::bounded::<DspRunner>(1);
        let _ = runner_tx.send(DspRunner::new(initial_graph, Some(midi_rx.clone())));
        
        let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

        let mut current_runner: Option<DspRunner> = None;
        let mut global_sample_index: u64 = 0;

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    if let Ok(new_runner) = runner_rx.try_recv() {
                        current_runner = Some(new_runner);
                    }

                    let Some(runner) = &mut current_runner else {
                        data.fill(0.0);
                        return;
                    };

                    // Process Parameter Updates
                    while let Ok(update) = param_rx.try_recv() {
                        runner.update_parameter(update.node_id, &update.param, update.value);
                    }

                    for frame in data.chunks_mut(channels) {
                        let ctx = ProcessContext {
                            sample_rate,
                            global_sample_index,
                        };
                        
                        let out = runner.process_sample(&ctx);
                        for (i, val) in out.iter().enumerate() {
                            if i < frame.len() {
                                frame[i] = *val;
                            }
                        }
                        global_sample_index += 1;
                        global_sample_index_atomic.store(global_sample_index, Ordering::Relaxed);
                    }
                },
                err_fn,
                None,
            )?,
            _ => return Err("Unsupported sample format".into()),
        };

        stream.play()?;

        Ok(Self { runner_tx, param_tx, midi_rx, crash_flag, _midi_conn, _stream: stream })
    }

    pub fn check_crash(&self) -> bool {
        self.crash_flag.swap(false, Ordering::SeqCst)
    }

    pub fn update_parameter(&self, node_id: StableId, param: &str, value: f32) {
        let _ = self.param_tx.send(ParameterUpdate {
            node_id,
            param: param.to_string(),
            value,
        });
    }

    pub fn update_graph(&self, graph: Graph) {
        let _ = self.runner_tx.send(DspRunner::new(graph, Some(self.midi_rx.clone())));
    }
}

