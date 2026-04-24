use dirtydata_core::types::ConfigSnapshot;
use rand::prelude::*;
use rand_pcg::Pcg32;
use std::sync::Arc;

/// Contextual information for the current processing sample.
pub struct ProcessContext {
    pub sample_rate: f32,
    pub global_sample_index: u64,
}

/// The fundamental trait for a DSP node.
/// Operates in the Sample Domain (one stereo sample at a time).
pub trait DspNode: Send + Sync {
    /// Process one stereo sample.
    /// inputs: flattened stereo samples [L1, R1, L2, R2, ...]
    /// outputs: slice of stereo pairs [[Lout1, Rout1], [Lout2, Rout2], ...]
    fn process(&mut self, inputs: &[f32], outputs: &mut [[f32; 2]], config: &ConfigSnapshot, ctx: &ProcessContext);
}

// ──────────────────────────────────────────────
// §1 — Sources
// ──────────────────────────────────────────────

pub struct OscillatorNode {
    phase: f32,
}

impl OscillatorNode {
    pub fn new() -> Self {
        Self { phase: 0.0 }
    }
}

impl DspNode for OscillatorNode {
    fn process(&mut self, _inputs: &[f32], outputs: &mut [[f32; 2]], config: &ConfigSnapshot, ctx: &ProcessContext) {
        let freq = config.get("frequency").and_then(|v| v.as_float()).unwrap_or(440.0) as f32;
        let wave_type = config.get("waveform").and_then(|v| v.as_string());
        
        let phase_inc = freq / ctx.sample_rate;
        
        let val = match wave_type.map(|s| s.as_str()).unwrap_or("sine") {
            "sine" => (self.phase * 2.0 * std::f32::consts::PI).sin(),
            "saw" => (self.phase * 2.0) - 1.0,
            "square" => if self.phase < 0.5 { 1.0 } else { -1.0 },
            "triangle" => {
                let v = self.phase * 4.0;
                if v < 1.0 { v - 0.0 }
                else if v < 3.0 { 2.0 - v }
                else { v - 4.0 }
            }
            _ => (self.phase * 2.0 * std::f32::consts::PI).sin(),
        };

        outputs[0][0] = val;
        outputs[0][1] = val;

        self.phase = (self.phase + phase_inc) % 1.0;
    }
}

pub struct NoiseNode {
    rng: Pcg32,
}

impl NoiseNode {
    pub fn new(seed: u64) -> Self {
        Self { rng: Pcg32::seed_from_u64(seed) }
    }
}

impl DspNode for NoiseNode {
    fn process(&mut self, _inputs: &[f32], outputs: &mut [[f32; 2]], _config: &ConfigSnapshot, _ctx: &ProcessContext) {
        let val: f32 = self.rng.random_range(-1.0..1.0);
        outputs[0][0] = val;
        outputs[0][1] = val;
    }
}

pub struct AssetReaderNode {
    data: Arc<Vec<f32>>,
    cursor: usize,
}

impl AssetReaderNode {
    pub fn new(data: Arc<Vec<f32>>) -> Self {
        Self { data, cursor: 0 }
    }
}

impl DspNode for AssetReaderNode {
    fn process(&mut self, _inputs: &[f32], outputs: &mut [[f32; 2]], _config: &ConfigSnapshot, _ctx: &ProcessContext) {
        if self.cursor + 1 < self.data.len() {
            outputs[0][0] = self.data[self.cursor];
            outputs[0][1] = self.data[self.cursor + 1];
            self.cursor += 2;
        } else {
            outputs[0][0] = 0.0;
            outputs[0][1] = 0.0;
        }
    }
}

// ──────────────────────────────────────────────
// §2 — Processors
// ──────────────────────────────────────────────

pub struct GainNode;

impl DspNode for GainNode {
    fn process(&mut self, inputs: &[f32], outputs: &mut [[f32; 2]], config: &ConfigSnapshot, _ctx: &ProcessContext) {
        let gain_db = config.get("gain_db").and_then(|v| v.as_float()).unwrap_or(0.0) as f32;
        let linear = 10.0_f32.powf(gain_db / 20.0);
        
        if inputs.len() >= 2 {
            outputs[0][0] = inputs[0] * linear;
            outputs[0][1] = inputs[1] * linear;
        }
    }
}

pub struct BiquadFilterNode {
    z1: [f32; 2],
    z2: [f32; 2],
}

impl BiquadFilterNode {
    pub fn new() -> Self {
        Self { z1: [0.0, 0.0], z2: [0.0, 0.0] }
    }
}

impl DspNode for BiquadFilterNode {
    fn process(&mut self, inputs: &[f32], outputs: &mut [[f32; 2]], config: &ConfigSnapshot, ctx: &ProcessContext) {
        let freq = config.get("frequency").and_then(|v| v.as_float()).unwrap_or(1000.0) as f32;
        let q = config.get("q").and_then(|v| v.as_float()).unwrap_or(0.707) as f32;
        let filter_type = config.get("type").and_then(|v| v.as_string());

        // Simple RBJ Biquad coefficients
        let w0 = 2.0 * std::f32::consts::PI * freq / ctx.sample_rate;
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();

        let (b0, b1, b2, a0, a1, a2) = match filter_type.map(|s| s.as_str()).unwrap_or("lpf") {
            "hpf" => {
                let b0 = (1.0 + cos_w0) / 2.0;
                let b1 = -(1.0 + cos_w0);
                let b2 = (1.0 + cos_w0) / 2.0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                (b0, b1, b2, a0, a1, a2)
            }
            _ => { // LPF
                let b0 = (1.0 - cos_w0) / 2.0;
                let b1 = 1.0 - cos_w0;
                let b2 = (1.0 - cos_w0) / 2.0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                (b0, b1, b2, a0, a1, a2)
            }
        };

        let inv_a0 = 1.0 / a0;
        let ff0 = b0 * inv_a0;
        let ff1 = b1 * inv_a0;
        let ff2 = b2 * inv_a0;
        let fb1 = a1 * inv_a0;
        let fb2 = a2 * inv_a0;

        for i in 0..2 {
            let x = if inputs.len() > i { inputs[i] } else { 0.0 };
            let y = ff0 * x + self.z1[i];
            self.z1[i] = ff1 * x - fb1 * y + self.z2[i];
            self.z2[i] = ff2 * x - fb2 * y;
            outputs[0][i] = y;
        }
    }
}

pub struct DelayNode {
    buffer: Vec<[f32; 2]>,
    write_pos: usize,
}

impl DelayNode {
    pub fn new(max_delay_samples: usize) -> Self {
        Self {
            buffer: vec![[0.0, 0.0]; max_delay_samples],
            write_pos: 0,
        }
    }
}

impl DspNode for DelayNode {
    fn process(&mut self, inputs: &[f32], outputs: &mut [[f32; 2]], config: &ConfigSnapshot, _ctx: &ProcessContext) {
        let delay_samples = config.get("delay_samples").and_then(|v| v.as_float()).unwrap_or(4410.0) as usize;
        let feedback = config.get("feedback").and_then(|v| v.as_float()).unwrap_or(0.5) as f32;
        
        let read_pos = (self.write_pos + self.buffer.len() - delay_samples) % self.buffer.len();
        let delayed = self.buffer[read_pos];
        
        outputs[0][0] = delayed[0];
        outputs[0][1] = delayed[1];

        let in_l = if inputs.len() >= 1 { inputs[0] } else { 0.0 };
        let in_r = if inputs.len() >= 2 { inputs[1] } else { 0.0 };

        self.buffer[self.write_pos] = [
            in_l + delayed[0] * feedback,
            in_r + delayed[1] * feedback,
        ];
        
        self.write_pos = (self.write_pos + 1) % self.buffer.len();
    }
}

// ──────────────────────────────────────────────
// §3 — Math
// ──────────────────────────────────────────────

pub struct AddNode;

impl DspNode for AddNode {
    fn process(&mut self, inputs: &[f32], outputs: &mut [[f32; 2]], _config: &ConfigSnapshot, _ctx: &ProcessContext) {
        // Sum all stereo input pairs
        let mut l = 0.0;
        let mut r = 0.0;
        for chunk in inputs.chunks_exact(2) {
            l += chunk[0];
            r += chunk[1];
        }
        outputs[0][0] = l;
        outputs[0][1] = r;
    }
}

pub struct MultiplyNode;

impl DspNode for MultiplyNode {
    fn process(&mut self, inputs: &[f32], outputs: &mut [[f32; 2]], _config: &ConfigSnapshot, _ctx: &ProcessContext) {
        if inputs.len() >= 4 {
            outputs[0][0] = inputs[0] * inputs[2];
            outputs[0][1] = inputs[1] * inputs[3];
        } else {
            outputs[0][0] = 0.0;
            outputs[0][1] = 0.0;
        }
    }
}

pub struct ClipNode;

impl DspNode for ClipNode {
    fn process(&mut self, inputs: &[f32], outputs: &mut [[f32; 2]], config: &ConfigSnapshot, _ctx: &ProcessContext) {
        let min = config.get("min").and_then(|v| v.as_float()).unwrap_or(-1.0) as f32;
        let max = config.get("max").and_then(|v| v.as_float()).unwrap_or(1.0) as f32;
        
        if inputs.len() >= 2 {
            outputs[0][0] = inputs[0].clamp(min, max);
            outputs[0][1] = inputs[1].clamp(min, max);
        }
    }
}

// ──────────────────────────────────────────────
// §4 — Alchemy (Modulation & Time)
// ──────────────────────────────────────────────

pub struct TriggerNode;

impl DspNode for TriggerNode {
    fn process(&mut self, _inputs: &[f32], outputs: &mut [[f32; 2]], config: &ConfigSnapshot, ctx: &ProcessContext) {
        let trigger_sample = config.get("sample").and_then(|v| v.as_float()).unwrap_or(0.0) as u64;
        let val = if ctx.global_sample_index == trigger_sample { 1.0 } else { 0.0 };
        outputs[0][0] = val;
        outputs[0][1] = val;
    }
}

#[derive(Clone, Copy)]
enum EnvState { Idle, Attack, Decay, Sustain, Release }

pub struct EnvelopeNode {
    state: EnvState,
    level: f32,
}

impl EnvelopeNode {
    pub fn new() -> Self {
        Self { state: EnvState::Idle, level: 0.0 }
    }
}

impl DspNode for EnvelopeNode {
    fn process(&mut self, inputs: &[f32], outputs: &mut [[f32; 2]], config: &ConfigSnapshot, ctx: &ProcessContext) {
        let a = config.get("attack").and_then(|v| v.as_float()).unwrap_or(0.1) as f32;
        let d = config.get("decay").and_then(|v| v.as_float()).unwrap_or(0.1) as f32;
        let s = config.get("sustain").and_then(|v| v.as_float()).unwrap_or(0.5) as f32;
        let r = config.get("release").and_then(|v| v.as_float()).unwrap_or(0.5) as f32;

        let gate = inputs.get(0).cloned().unwrap_or(0.0) > 0.0;

        match self.state {
            EnvState::Idle => {
                if gate { self.state = EnvState::Attack; }
            }
            EnvState::Attack => {
                if !gate { self.state = EnvState::Release; }
                else {
                    self.level += 1.0 / (a * ctx.sample_rate);
                    if self.level >= 1.0 {
                        self.level = 1.0;
                        self.state = EnvState::Decay;
                    }
                }
            }
            EnvState::Decay => {
                if !gate { self.state = EnvState::Release; }
                else {
                    self.level -= (1.0 - s) / (d * ctx.sample_rate);
                    if self.level <= s {
                        self.level = s;
                        self.state = EnvState::Sustain;
                    }
                }
            }
            EnvState::Sustain => {
                if !gate { self.state = EnvState::Release; }
            }
            EnvState::Release => {
                if gate { self.state = EnvState::Attack; }
                else {
                    self.level -= s / (r * ctx.sample_rate);
                    if self.level <= 0.0 {
                        self.level = 0.0;
                        self.state = EnvState::Idle;
                    }
                }
            }
        }

        outputs[0][0] = self.level;
        outputs[0][1] = self.level;
    }
}

pub struct AutomationNode;

impl DspNode for AutomationNode {
    fn process(&mut self, _inputs: &[f32], outputs: &mut [[f32; 2]], config: &ConfigSnapshot, ctx: &ProcessContext) {
        let keyframes = config.get("keyframes").and_then(|v| v.as_list());
        let current_time = ctx.global_sample_index as f64 / ctx.sample_rate as f64;

        let mut val = 0.0;

        if let Some(keys) = keyframes {
            let mut prev_t = 0.0;
            let mut prev_v = 0.0;
            let mut found = false;

            for key in keys {
                if let Some(pair) = key.as_list() {
                    if pair.len() >= 2 {
                        let t = pair[0].as_float().unwrap_or(0.0);
                        let v = pair[1].as_float().unwrap_or(0.0) as f32;

                        if current_time < t {
                            let dt = t - prev_t;
                            if dt > 0.0 {
                                let frac = ((current_time - prev_t) / dt) as f32;
                                val = prev_v + (v - prev_v) * frac;
                            } else {
                                val = v;
                            }
                            found = true;
                            break;
                        }
                        prev_t = t;
                        prev_v = v;
                    }
                }
            }
            if !found {
                val = prev_v;
            }
        }

        outputs[0][0] = val;
        outputs[0][1] = val;
    }
}

pub struct MidiEvent {
    pub sample_index: u64,
    pub message: [u8; 3],
}

pub struct MidiInNode {
    event_rx: crossbeam_channel::Receiver<MidiEvent>,
    gate: f32,
    pitch_hz: f32,
    velocity: f32,
    pending_events: Vec<MidiEvent>,
}

impl MidiInNode {
    pub fn new(event_rx: crossbeam_channel::Receiver<MidiEvent>) -> Self {
        Self {
            event_rx,
            gate: 0.0,
            pitch_hz: 440.0,
            velocity: 0.0,
            pending_events: Vec::new(),
        }
    }
}

impl DspNode for MidiInNode {
    fn process(&mut self, _inputs: &[f32], outputs: &mut [[f32; 2]], _config: &ConfigSnapshot, ctx: &ProcessContext) {
        // 1. Drain queue into pending
        while let Ok(event) = self.event_rx.try_recv() {
            self.pending_events.push(event);
        }

        // 2. Process events for current sample
        self.pending_events.retain(|event| {
            if event.sample_index <= ctx.global_sample_index {
                let status = event.message[0] & 0xF0;
                match status {
                    0x90 => { // Note On
                        let note = event.message[1];
                        let vel = event.message[2];
                        if vel > 0 {
                            self.gate = 1.0;
                            self.pitch_hz = 440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0);
                            self.velocity = vel as f32 / 127.0;
                        } else {
                            self.gate = 0.0;
                        }
                    }
                    0x80 => { // Note Off
                        self.gate = 0.0;
                    }
                    _ => {}
                }
                false // Handled
            } else {
                true // Future
            }
        });

        // Port 0: Gate
        outputs[0][0] = self.gate;
        outputs[0][1] = self.gate;
        // Port 1: Pitch
        if outputs.len() > 1 {
            outputs[1][0] = self.pitch_hz;
            outputs[1][1] = self.pitch_hz;
        }
        // Port 2: Velocity
        if outputs.len() > 2 {
            outputs[2][0] = self.velocity;
            outputs[2][1] = self.velocity;
        }
    }
}



