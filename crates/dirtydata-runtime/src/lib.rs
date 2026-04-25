pub mod nodes;

use arc_swap::ArcSwap;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use dirtydata_core::ir::Graph;
use dirtydata_core::types::{StableId, NodeKind};
use dirtydata_core::{graph_utils, ConfigSnapshot};
use crate::nodes::{DspNode, OscillatorNode, GainNode, AddNode, MultiplyNode, NoiseNode, ClipNode, BiquadFilterNode, DelayNode, AssetReaderNode, TriggerNode, EnvelopeNode, AutomationNode, ProcessContext, MidiInNode, MidiEvent, SequencerNode};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use serde_json;

pub struct ParameterUpdate {
    pub node_id: StableId,
    pub param: String,
    pub value: f32,
}

pub mod offline;
pub use offline::OfflineRenderer;
use crate::nodes::{NodeState};

pub enum EngineCommand {
    UpdateParameter(ParameterUpdate),
    ReplaceGraph(Graph),
}

/// A stateful runner for the DSP graph.
pub(crate) struct DspRunner {
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
                    "Sequencer" => Box::new(SequencerNode::new()),
                    "Automation" => Box::new(AutomationNode),
                    "MidiIn" => {
                        if let Some(rx) = &midi_rx {
                            Box::new(MidiInNode::new(rx.clone()))
                        } else {
                            Box::new(GainNode::new())
                        }
                    }
                    "VoiceStack" => {
                        let template_str = node.config.get("template").and_then(|v| v.as_string()).map(|s| s.as_str()).unwrap_or("{}");
                        let template_graph: Graph = serde_json::from_str(&template_str).unwrap_or_default();
                        Box::new(VoiceStackNode::new(template_graph, 8))
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

    pub fn update_all_nodes(&mut self, param: &str, value: f32) {
        for (_, node) in &mut self.nodes {
            node.update_parameter(param, value);
        }
    }

    pub fn extract_all_states(&self) -> HashMap<StableId, NodeState> {
        let mut states = HashMap::new();
        for (id, node) in &self.nodes {
            states.insert(*id, node.extract_state());
        }
        states
    }

    pub fn inject_all_states(&mut self, states: &HashMap<StableId, NodeState>) {
        for (id, node) in &mut self.nodes {
            if let Some(state) = states.get(id) {
                node.inject_state(state);
            }
        }
    }
}

struct VoiceSlot {
    runner: DspRunner,
    note: Option<u8>,
    last_on: u64,
    pending_trigger: Option<(u8, u8)>,
}

pub struct VoiceStackNode {
    slots: Vec<VoiceSlot>,
}

impl VoiceStackNode {
    pub fn new(template_graph: Graph, count: usize) -> Self {
        let mut slots = Vec::new();
        for _ in 0..count {
            slots.push(VoiceSlot {
                runner: DspRunner::new(template_graph.clone(), None),
                note: None,
                last_on: 0,
                pending_trigger: None,
            });
        }
        Self { slots }
    }
}

impl DspNode for VoiceStackNode {
    fn process(&mut self, inputs: &[f32], outputs: &mut [[f32; 2]], _config: &ConfigSnapshot, ctx: &ProcessContext) {
        // Decode CV-Command Protocol from Port 0
        if inputs.len() >= 2 {
            let cmd = inputs[0].round();
            let data = inputs[1].round() as u32;
            
            if cmd == 1.0 {
                // Note On
                let note = (data >> 8) as u8;
                let vel = (data & 0xFF) as u8;
                self.update_parameter("note_on", ((vel as u32) << 8 | (note as u32)) as f32);
            } else if cmd == 2.0 {
                // Note Off (Simplification: note info might be in data)
                let note = (data >> 8) as u8;
                self.update_parameter("note_off", note as f32);
            }
        }

        // Sum outputs
        outputs[0] = [0.0, 0.0];

        for (i, slot) in self.slots.iter_mut().enumerate() {
            // If pending trigger, wait for idle (after steal)
            if let Some((note, vel)) = slot.pending_trigger {
                // How to check if idle? 
                // We'll assume the runner nodes update their state.
                // For now, let's just trigger after 5ms (FastRelease time)
                // This is a bit simplified; in a real engine we'd check Envelope state.
                // But for the proof of concept:
                slot.runner.update_all_nodes("frequency", 440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0));
                slot.runner.update_all_nodes("velocity", vel as f32 / 127.0);
                slot.runner.update_all_nodes("gate", 1.0);
                slot.note = Some(note);
                slot.last_on = ctx.global_sample_index;
                slot.pending_trigger = None;
            }

            let out = slot.runner.process_sample(ctx);
            outputs[0][0] += out[0];
            outputs[0][1] += out[1];
        }
    }

    fn update_parameter(&mut self, param: &str, value: f32) {
        // Handle MIDI-like commands or broadcast global params
        if param == "note_on" {
            let note = (value as u32 & 0xFF) as u8;
            let vel = ((value as u32 >> 8) & 0xFF) as u8;
            
            // Allocation logic
            // 1. Find idle slot
            let mut target_idx = None;
            for (i, slot) in self.slots.iter().enumerate() {
                if slot.note.is_none() {
                    target_idx = Some(i);
                    break;
                }
            }
            
            // 2. Steal if no idle
            if target_idx.is_none() {
                let mut oldest_idx = 0;
                let mut oldest_time = u64::MAX;
                for (i, slot) in self.slots.iter().enumerate() {
                    if slot.last_on < oldest_time {
                        oldest_time = slot.last_on;
                        oldest_idx = i;
                    }
                }
                target_idx = Some(oldest_idx);
                // Trigger FastRelease
                self.slots[oldest_idx].runner.update_all_nodes("steal", 1.0);
            }
            
            if let Some(idx) = target_idx {
                self.slots[idx].pending_trigger = Some((note, vel));
            }
        } else if param == "note_off" {
            let note = value as u8;
            for slot in &mut self.slots {
                if slot.note == Some(note) {
                    slot.runner.update_all_nodes("gate", 0.0);
                    slot.note = None;
                }
            }
        } else {
            // Broadcast global parameter to all voices
            for slot in &mut self.slots {
                slot.runner.update_all_nodes(param, value);
            }
        }
    }
}

pub struct AudioEngine {
    command_tx: crossbeam_channel::Sender<EngineCommand>,
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
        let (command_tx, command_rx) = crossbeam_channel::unbounded::<EngineCommand>();
        let _ = command_tx.send(EngineCommand::ReplaceGraph(initial_graph));
        
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

        let _err_fn = |err| eprintln!("an error occurred on stream: {}", err);

        let mut current_runner: Option<DspRunner> = None;
        let mut global_sample_index: u64 = 0;
        let midi_rx_internal = midi_rx.clone();

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    // Process Engine Commands
                    while let Ok(cmd) = command_rx.try_recv() {
                        match cmd {
                            EngineCommand::UpdateParameter(update) => {
                                if let Some(runner) = &mut current_runner {
                                    runner.update_parameter(update.node_id, &update.param, update.value);
                                }
                            }
                            EngineCommand::ReplaceGraph(graph) => {
                                let mut new_runner = DspRunner::new(graph, Some(midi_rx_internal.clone()));
                                if let Some(old_runner) = &current_runner {
                                    let states = old_runner.extract_all_states();
                                    new_runner.inject_all_states(&states);
                                }
                                current_runner = Some(new_runner);
                            }
                        }
                    }

                    let Some(runner) = &mut current_runner else {
                        data.fill(0.0);
                        return;
                    };

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
                _err_fn,
                None,
            )?,
            _ => return Err("Unsupported sample format".into()),
        };

        stream.play()?;

        Ok(Self { command_tx, midi_rx, crash_flag, _midi_conn, _stream: stream })
    }

    pub fn check_crash(&self) -> bool {
        self.crash_flag.swap(false, Ordering::SeqCst)
    }

    pub fn update_parameter(&self, node_id: StableId, param: &str, value: f32) {
        let _ = self.command_tx.send(EngineCommand::UpdateParameter(ParameterUpdate {
            node_id,
            param: param.to_string(),
            value,
        }));
    }

    pub fn update_graph(&self, graph: Graph) {
        let _ = self.command_tx.send(EngineCommand::ReplaceGraph(graph));
    }
}

