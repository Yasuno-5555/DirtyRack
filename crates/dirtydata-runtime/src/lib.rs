pub mod nodes;

use crate::nodes::{
    AddNode, AssetReaderNode, AutomationNode, BiquadFilterNode, ClipNode, ClockNode,
    CompressorNode, DelayNode, DspNode, EnvelopeNode, FFTConvolveNode, FeedbackNode, ForeignNode,
    GainNode, GranularNode, GrayScottNode, InputProxyNode, LogicNode, LorenzNode, MackeyGlassNode,
    MidiEvent, MidiInNode, MultiplyNode, NodeState, NoiseNode, OscMessage, OscOutNode,
    OscillatorNode, OutputProxyNode, ProbabilityGateNode, ProcessContext, ReverbNode,
    SampleHoldNode, SequencerNode, SlewLimiterNode, SpectralFreezeNode, SubGraphNode, TriggerNode,
    WasmNode, WavefolderNode,
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use dirtydata_core::graph_utils::topological_sort;
use dirtydata_core::ir::{EdgeKind, Graph};
use dirtydata_core::types::{NodeKind, PortDirection, StableId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct ParameterUpdate {
    pub node_id: StableId,
    pub param: String,
    pub value: f32,
}

pub mod offline;
pub use offline::OfflineRenderer;

pub enum EngineCommand {
    UpdateParameter(ParameterUpdate),
    ReplaceGraph(Graph),
}

pub struct SharedState {
    pub node_levels: Arc<dashmap::DashMap<StableId, f32>>,
    pub scope_buffer: Arc<crossbeam_queue::ArrayQueue<f32>>,
    pub probe_buffers: Arc<dashmap::DashMap<StableId, Arc<crossbeam_queue::ArrayQueue<f32>>>>,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            node_levels: Arc::new(dashmap::DashMap::new()),
            scope_buffer: Arc::new(crossbeam_queue::ArrayQueue::new(1024)),
            probe_buffers: Arc::new(dashmap::DashMap::new()),
        }
    }
}

pub struct DspRunner {
    nodes: Vec<(StableId, Box<dyn DspNode>)>,
    pub node_outputs: HashMap<StableId, Vec<[f32; 2]>>,
    graph: Graph,
    feedback_latches: Vec<[f32; 2]>,
    feedback_reads: Vec<Vec<(usize, usize)>>,
    feedback_writes: Vec<Vec<(usize, usize)>>,
    modulation_mappings: Vec<ModulationMapping>,
}

struct ModulationMapping {
    source_node_id: StableId,
    source_port_idx: usize,
    target_node_idx: usize,
    target_param: String,
    amount: f32,
}

impl DspRunner {
    pub fn new(
        graph: Graph,
        midi_rx: Option<crossbeam_channel::Receiver<MidiEvent>>,
        sample_rate: f32,
    ) -> Self {
        let (sorted_ids, _) = topological_sort(&graph);
        let mut nodes: Vec<(StableId, Box<dyn DspNode>)> = Vec::new();
        let mut node_outputs = HashMap::new();

        for &id in &sorted_ids {
            if let Some(node) = graph.nodes.get(&id) {
                let dsp_node: Box<dyn DspNode> = match &node.kind {
                    NodeKind::Foreign(plugin_name) => {
                        Box::new(ForeignNode::new(plugin_name.clone(), 256))
                    }
                    _ => {
                        let name = node.config.get("name").and_then(|v| v.as_string());
                        match name.map(|s| s.as_str()).unwrap_or("Unknown") {
                            "Oscillator" | "Sine" => Box::new(OscillatorNode::new()),
                            "Noise" => {
                                Box::new(NoiseNode::new(format!("{}", id).as_bytes().len() as u64))
                            }
                            "Gain" => Box::new(GainNode::new()),
                            "Add" => Box::new(AddNode::new()),
                            "Multiply" => Box::new(MultiplyNode::new()),
                            "Clip" => Box::new(ClipNode::new()),
                            "Filter" | "Biquad" => Box::new(BiquadFilterNode::new()),
                            "Compressor" | "Dynamics" => Box::new(CompressorNode::new()),
                            "Delay" => Box::new(DelayNode::new(sample_rate as usize)),
                            "Sampler" | "AssetReader" => {
                                Box::new(AssetReaderNode::new(Arc::new(vec![])))
                            }
                            "Trigger" => Box::new(TriggerNode::new()),
                            "Envelope" | "ADSR" => Box::new(EnvelopeNode::new()),
                            "Automation" => Box::new(AutomationNode::new()),
                            "MidiIn" => {
                                if let Some(rx) = &midi_rx {
                                    Box::new(MidiInNode::new(rx.clone()))
                                } else {
                                    Box::new(GainNode::new())
                                }
                            }
                            "Sequencer" => Box::new(SequencerNode::new()),
                            "Wavefolder" => Box::new(WavefolderNode::new()),
                            "Lorenz" => Box::new(LorenzNode::new()),
                            "MackeyGlass" => Box::new(MackeyGlassNode::new(30.0, sample_rate)),
                            "GrayScott" => Box::new(GrayScottNode::new(100)),
                            "SlewLimiter" => Box::new(SlewLimiterNode::new()),
                            "SampleHold" => Box::new(SampleHoldNode::new()),
                            "Clock" => Box::new(ClockNode::new()),
                            "ProbabilityGate" => Box::new(ProbabilityGateNode::new()),
                            "Reverb" => Box::new(ReverbNode::new(sample_rate)),
                            "Granular" => Box::new(GranularNode::new(sample_rate)),
                            "Wasm" => Box::new(WasmNode::new()),
                            "Logic" => Box::new(LogicNode::new()),
                            "SpectralFreeze" => Box::new(SpectralFreezeNode::new(2048)),
                            "FFTConvolve" => Box::new(FFTConvolveNode::new(2048)),
                            "Feedback" => Box::new(FeedbackNode::new()),
                            "OscOut" => Box::new(OscOutNode::new()),
                            "SubGraph" => Box::new(SubGraphNode::new()),
                            "InputProxy" => Box::new(InputProxyNode::new()),
                            "OutputProxy" => Box::new(OutputProxyNode::new()),
                            _ => Box::new(GainNode::new()),
                        }
                    }
                };
                nodes.push((id, dsp_node));
                let port_count = node
                    .ports
                    .iter()
                    .filter(|p| p.direction == PortDirection::Output)
                    .count()
                    .max(1);
                node_outputs.insert(id, vec![[0.0, 0.0]; port_count]);
            }
        }

        let mut feedback_latches = Vec::new();
        let mut feedback_reads = vec![Vec::new(); nodes.len()];
        let mut feedback_writes = vec![Vec::new(); nodes.len()];

        for edge in graph.edges.values() {
            if edge.kind == EdgeKind::Feedback {
                let latch_idx = feedback_latches.len();
                feedback_latches.push([0.0, 0.0]);
                if let Some(src_idx) = nodes.iter().position(|(id, _)| *id == edge.source.node_id) {
                    feedback_writes[src_idx].push((0, latch_idx));
                }
                if let Some(tgt_idx) = nodes.iter().position(|(id, _)| *id == edge.target.node_id) {
                    feedback_reads[tgt_idx].push((0, latch_idx));
                }
            }
        }

        let mut modulation_mappings = Vec::new();
        for m in graph.modulations.values() {
            if let Some(target_idx) = nodes.iter().position(|(id, _)| *id == m.target_node) {
                modulation_mappings.push(ModulationMapping {
                    source_node_id: m.source.node_id,
                    source_port_idx: 0,
                    target_node_idx: target_idx,
                    target_param: m.target_param.clone(),
                    amount: m.amount,
                });
            }
        }

        Self {
            nodes,
            node_outputs,
            graph,
            feedback_latches,
            feedback_reads,
            feedback_writes,
            modulation_mappings,
        }
    }

    pub fn process_sample(&mut self, ctx: &ProcessContext) -> [f32; 2] {
        for m in &self.modulation_mappings {
            if let Some(outputs) = self.node_outputs.get(&m.source_node_id) {
                let val = (outputs[m.source_port_idx][0] + outputs[m.source_port_idx][1]) * 0.5;
                let (_, node) = &mut self.nodes[m.target_node_idx];
                node.update_parameter(&m.target_param, val * m.amount);
            }
        }

        for (i, (id, node)) in self.nodes.iter_mut().enumerate() {
            let mut inputs = Vec::new();
            for edge in self.graph.edges.values() {
                if edge.kind == EdgeKind::Normal && edge.target.node_id == *id {
                    if let Some(prev_outputs) = self.node_outputs.get(&edge.source.node_id) {
                        let val = prev_outputs[0];
                        inputs.push(val[0]);
                        inputs.push(val[1]);
                    }
                }
            }

            for (_, latch_idx) in &self.feedback_reads[i] {
                let latch = self.feedback_latches[*latch_idx];
                if inputs.is_empty() {
                    inputs.push(latch[0]);
                    inputs.push(latch[1]);
                }
            }

            let outputs = self.node_outputs.get_mut(id).unwrap();
            node.process(
                &inputs,
                &mut outputs[..],
                &self.graph.nodes.get(id).unwrap().config,
                ctx,
            );

            for (_, latch_idx) in &self.feedback_writes[i] {
                let val: [f32; 2] = outputs[0];
                self.feedback_latches[*latch_idx] = val;
            }
        }

        let mut final_out = [0.0, 0.0];
        for (id, _) in &self.nodes {
            if let Some(node) = self.graph.nodes.get(id) {
                if node.kind == NodeKind::Sink {
                    let out = self.node_outputs.get(id).unwrap()[0];
                    final_out[0] += out[0];
                    final_out[1] += out[1];
                }
            }
        }
        final_out
    }

    pub fn update_parameter(&mut self, node_id: StableId, param: &str, value: f32) {
        if let Some((_, node)) = self.nodes.iter_mut().find(|(id, _)| *id == node_id) {
            node.update_parameter(param, value);
        }
    }

    pub fn nodes_mut(&mut self) -> &mut Vec<(StableId, Box<dyn DspNode>)> {
        &mut self.nodes
    }

    pub fn get_graph(&self) -> &Graph {
        &self.graph
    }

    pub fn extract_all_states(&self) -> HashMap<StableId, NodeState> {
        self.nodes
            .iter()
            .map(|(id, node)| (*id, node.extract_state()))
            .collect()
    }

    pub fn inject_all_states(&mut self, states: &HashMap<StableId, NodeState>) {
        for (id, node) in &mut self.nodes {
            if let Some(state) = states.get(id) {
                node.inject_state(state);
            }
        }
    }
}

pub struct AudioEngine {
    _stream: cpal::Stream,
    pub command_tx: crossbeam_channel::Sender<EngineCommand>,
    pub shared_state: Arc<SharedState>,
}

impl AudioEngine {
    pub fn new(
        shared_state: Arc<SharedState>,
        midi_rx: crossbeam_channel::Receiver<MidiEvent>,
    ) -> Self {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .expect("no output device available");
        let config = device.default_output_config().unwrap();
        let sample_rate = config.sample_rate().0 as f32;
        let channels = config.channels() as usize;

        let (command_tx, command_rx) = crossbeam_channel::unbounded::<EngineCommand>();
        let shared_state_for_audio = shared_state.clone();
        let crash_flag_for_audio = Arc::new(AtomicBool::new(false));

        let command_tx_for_osc = command_tx.clone();
        std::thread::spawn(move || {
            use rosc::OscPacket;
            use std::net::UdpSocket;
            let socket = match UdpSocket::bind("127.0.0.1:8000") {
                Ok(s) => s,
                Err(_) => return,
            };
            let mut buf = [0u8; 4096];
            loop {
                match socket.recv_from(&mut buf) {
                    Ok((size, _)) => {
                        if let Ok((_, packet)) = rosc::decoder::decode_udp(&buf[..size]) {
                            match packet {
                                OscPacket::Message(msg) => {
                                    let parts: Vec<&str> =
                                        msg.addr.split('/').filter(|s| !s.is_empty()).collect();
                                    if parts.len() == 3 && parts[0] == "node" {
                                        if let (Ok(node_id), Some(val)) =
                                            (parts[1].parse::<StableId>(), msg.args.first())
                                        {
                                            let float_val = match val {
                                                rosc::OscType::Float(f) => *f,
                                                rosc::OscType::Double(d) => *d as f32,
                                                rosc::OscType::Int(i) => *i as f32,
                                                _ => 0.0,
                                            };
                                            let _ = command_tx_for_osc.send(
                                                EngineCommand::UpdateParameter(ParameterUpdate {
                                                    node_id,
                                                    param: parts[2].to_string(),
                                                    value: float_val,
                                                }),
                                            );
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        let (osc_tx, osc_rx) = crossbeam_channel::bounded::<OscMessage>(1024);
        std::thread::spawn(move || {
            use rosc::{OscMessage as RoscMessage, OscPacket};
            use std::net::UdpSocket;
            let socket = match UdpSocket::bind("0.0.0.0:0") {
                Ok(s) => s,
                Err(_) => return,
            };
            while let Ok(msg) = osc_rx.recv() {
                let packet = OscPacket::Message(RoscMessage {
                    addr: msg.addr,
                    args: msg.args,
                });
                if let Ok(bytes) = rosc::encoder::encode(&packet) {
                    let _ = socket.send_to(&bytes, "127.0.0.1:9001");
                }
            }
        });

        let mut current_runner: Option<DspRunner> = None;
        let mut global_sample_index: u64 = 0;
        let midi_rx_internal = midi_rx.clone();
        let crash_flag_callback = crash_flag_for_audio.clone();

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device
                .build_output_stream(
                    &config.into(),
                    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        while let Ok(cmd) = command_rx.try_recv() {
                            match cmd {
                                EngineCommand::UpdateParameter(update) => {
                                    if let Some(runner) = &mut current_runner {
                                        runner.update_parameter(
                                            update.node_id,
                                            &update.param,
                                            update.value,
                                        );
                                    }
                                }
                                EngineCommand::ReplaceGraph(graph) => {
                                    let mut new_runner = DspRunner::new(
                                        graph,
                                        Some(midi_rx_internal.clone()),
                                        sample_rate,
                                    );
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
                                crash_flag: Some(&crash_flag_callback),
                                osc_tx: Some(&osc_tx),
                            };
                            let out = runner.process_sample(&ctx);
                            for (node_id, ports) in &runner.node_outputs {
                                let peak = ports[0][0].abs().max(ports[0][1].abs());
                                shared_state_for_audio.node_levels.insert(*node_id, peak);

                                let probes = &shared_state_for_audio.probe_buffers;
                                if let Some(buf_ref) = probes.get(node_id) {
                                    let _ = buf_ref.value().push((ports[0][0] + ports[0][1]) * 0.5);
                                }
                                if peak.is_nan() {
                                    crash_flag_callback.store(true, Ordering::SeqCst);
                                }
                            }
                            frame[0] = out[0];
                            if channels > 1 {
                                frame[1] = out[1];
                            }
                            global_sample_index += 1;
                        }
                    },
                    |err| eprintln!("an error occurred on stream: {}", err),
                    None,
                )
                .unwrap(),
            _ => panic!("unsupported sample format"),
        };

        stream.play().unwrap();
        Self {
            _stream: stream,
            command_tx,
            shared_state,
        }
    }
}

pub struct VoiceStackNode {
    slots: Vec<VoiceSlot>,
}

struct VoiceSlot {
    id: Option<u8>,
    runner: DspRunner,
    active: bool,
    velocity: f32,
    last_on: u64,
}

impl VoiceStackNode {
    pub fn new(graph: Graph, voice_count: usize, sample_rate: f32) -> Self {
        let mut slots = Vec::new();
        for _ in 0..voice_count {
            slots.push(VoiceSlot {
                id: None,
                runner: DspRunner::new(graph.clone(), None, sample_rate),
                active: false,
                velocity: 0.0,
                last_on: 0,
            });
        }
        Self { slots }
    }

    pub fn process(&mut self, midi_events: &[MidiEvent], ctx: &ProcessContext) -> [f32; 2] {
        for event in midi_events {
            let status = event.message[0] & 0xF0;
            let note = event.message[1];
            let velocity = event.message[2];

            match status {
                0x90 if velocity > 0 => {
                    // Note On
                    if let Some(slot) = self.find_free_slot() {
                        slot.id = Some(note);
                        slot.active = true;
                        slot.velocity = velocity as f32 / 127.0;
                        slot.last_on = ctx.global_sample_index;
                        let freq = 440.0 * 2.0f32.powf((note as f32 - 69.0) / 12.0);
                        slot.runner
                            .update_parameter(StableId::new(), "frequency", freq);
                        slot.runner.update_parameter(StableId::new(), "gate", 1.0);
                    }
                }
                0x80 | 0x90 => {
                    // Note Off (or Note On with 0 velocity)
                    if let Some(slot) = self.slots.iter_mut().find(|s| s.id == Some(note)) {
                        slot.active = false;
                        slot.id = None;
                        slot.runner.update_parameter(StableId::new(), "gate", 0.0);
                    }
                }
                _ => {}
            }
        }

        let mut out = [0.0, 0.0];
        for slot in &mut self.slots {
            let s_out = slot.runner.process_sample(ctx);
            out[0] += s_out[0];
            out[1] += s_out[1];
        }
        out
    }

    fn find_free_slot(&mut self) -> Option<&mut VoiceSlot> {
        let mut best_idx = None;
        for (i, slot) in self.slots.iter().enumerate() {
            if !slot.active {
                best_idx = Some(i);
                break;
            }
        }
        if let Some(idx) = best_idx {
            return Some(&mut self.slots[idx]);
        }
        let oldest_idx = self
            .slots
            .iter()
            .enumerate()
            .min_by_key(|(_, s)| s.last_on)
            .map(|(i, _)| i);
        if let Some(idx) = oldest_idx {
            return Some(&mut self.slots[idx]);
        }
        None
    }
}
