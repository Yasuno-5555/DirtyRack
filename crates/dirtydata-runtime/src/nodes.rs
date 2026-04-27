use dirtydata_core::types::ConfigSnapshot;
use dirtydata_host::PluginHost;
use rand::prelude::*;
use rand_pcg::Pcg32;
use std::collections::VecDeque;
use std::sync::Arc;

/// A helper for smoothing parameter changes using a One-Pole LPF.
pub struct SmoothedValue {
    current: f32,
    target: f32,
    coeff: f32,
}

impl SmoothedValue {
    pub fn new(initial: f32, sample_rate: f32, time_constant_ms: f32) -> Self {
        // a = 1 - exp(-1 / (fs * tau))
        let tau = time_constant_ms * 0.001;
        let coeff = 1.0 - (-1.0 / (sample_rate * tau)).exp();
        Self {
            current: initial,
            target: initial,
            coeff,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn next(&mut self) -> f32 {
        self.current += self.coeff * (self.target - self.current);
        self.current
    }

    pub fn current(&self) -> f32 {
        self.current
    }
}

/// §7.1 — High-Precision Numerical Integration (RK4)
pub fn rk4_step<F>(state: &mut [f32], dt: f32, t: f32, derivative: F)
where
    F: Fn(&[f32], f32) -> Vec<f32>,
{
    let k1 = derivative(state, t);

    let mut s2 = state.to_vec();
    for i in 0..state.len() {
        s2[i] += k1[i] * dt * 0.5;
    }
    let k2 = derivative(&s2, t + dt * 0.5);

    let mut s3 = state.to_vec();
    for i in 0..state.len() {
        s3[i] += k2[i] * dt * 0.5;
    }
    let k3 = derivative(&s3, t + dt * 0.5);

    let mut s4 = state.to_vec();
    for i in 0..state.len() {
        s4[i] += k3[i] * dt;
    }
    let k4 = derivative(&s4, t + dt);

    for i in 0..state.len() {
        state[i] += (dt / 6.0) * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]);
    }
}

pub struct OscMessage {
    pub addr: String,
    pub args: Vec<rosc::OscType>,
}

/// Contextual information for the current processing sample.
pub struct ProcessContext<'a> {
    pub sample_rate: f32,
    pub global_sample_index: u64,
    pub crash_flag: Option<&'a std::sync::atomic::AtomicBool>,
    pub osc_tx: Option<&'a crossbeam_channel::Sender<OscMessage>>,
}

#[derive(Debug, Clone)]
pub enum NodeState {
    Empty,
    Oscillator { phase: f32 },
    Envelope { state_raw: u8, level: f32 },
}

pub trait DspNode: Send + Sync {
    /// Process one stereo sample.
    /// inputs: flattened stereo samples [L1, R1, L2, R2, ...]
    /// outputs: slice of stereo pairs [[Lout1, Rout1], [Lout2, Rout2], ...]
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        ctx: &ProcessContext,
    );

    /// Update a parameter in real-time.
    fn update_parameter(&mut self, _param: &str, _value: f32) {}

    fn extract_state(&self) -> NodeState {
        NodeState::Empty
    }
    fn inject_state(&mut self, _state: &NodeState) {}
}

// ──────────────────────────────────────────────
// §1 — Sources
// ──────────────────────────────────────────────

pub struct OscillatorNode {
    phase: f32,
    freq_smooth: Option<SmoothedValue>,
}

impl OscillatorNode {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            freq_smooth: None,
        }
    }
}

impl DspNode for OscillatorNode {
    fn process(
        &mut self,
        _inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        ctx: &ProcessContext,
    ) {
        let freq_target = config
            .get("frequency")
            .and_then(|v| v.as_float())
            .unwrap_or(440.0) as f32;
        let wave_type = config.get("waveform").and_then(|v| v.as_string());

        let smooth = self
            .freq_smooth
            .get_or_insert_with(|| SmoothedValue::new(freq_target, ctx.sample_rate, 10.0));
        let freq = smooth.next();
        let phase_inc = freq / ctx.sample_rate;

        let val = match wave_type.map(|s| s.as_str()).unwrap_or("sine") {
            "sine" => (self.phase * 2.0 * std::f32::consts::PI).sin(),
            "saw" => (self.phase * 2.0) - 1.0,
            "square" => {
                if self.phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            "triangle" => {
                let v = self.phase * 4.0;
                if v < 1.0 {
                    v - 0.0
                } else if v < 3.0 {
                    2.0 - v
                } else {
                    v - 4.0
                }
            }
            _ => (self.phase * 2.0 * std::f32::consts::PI).sin(),
        };

        outputs[0][0] = val;
        outputs[0][1] = val;

        self.phase = (self.phase + phase_inc) % 1.0;
    }

    fn update_parameter(&mut self, param: &str, value: f32) {
        if param == "frequency" {
            if let Some(s) = &mut self.freq_smooth {
                s.set_target(value);
            }
        }
    }

    fn extract_state(&self) -> NodeState {
        NodeState::Oscillator { phase: self.phase }
    }

    fn inject_state(&mut self, state: &NodeState) {
        if let NodeState::Oscillator { phase } = state {
            self.phase = *phase;
        }
    }
}

pub struct NoiseNode {
    rng: Pcg32,
}

impl NoiseNode {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Pcg32::seed_from_u64(seed),
        }
    }
}

impl DspNode for NoiseNode {
    fn process(
        &mut self,
        _inputs: &[f32],
        outputs: &mut [[f32; 2]],
        _config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
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
    fn process(
        &mut self,
        _inputs: &[f32],
        outputs: &mut [[f32; 2]],
        _config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
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

pub struct GainNode {
    gain_smooth: Option<SmoothedValue>,
}

impl GainNode {
    pub fn new() -> Self {
        Self { gain_smooth: None }
    }
}

impl DspNode for GainNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        ctx: &ProcessContext,
    ) {
        let gain_db_target = config
            .get("gain_db")
            .and_then(|v| v.as_float())
            .unwrap_or(0.0) as f32;

        let smooth = self
            .gain_smooth
            .get_or_insert_with(|| SmoothedValue::new(gain_db_target, ctx.sample_rate, 10.0));
        let gain_db = smooth.next();
        let linear = 10.0_f32.powf(gain_db / 20.0);

        if inputs.len() >= 2 {
            outputs[0][0] = inputs[0] * linear;
            outputs[0][1] = inputs[1] * linear;
        }
    }

    fn update_parameter(&mut self, param: &str, value: f32) {
        if param == "gain_db" {
            if let Some(s) = &mut self.gain_smooth {
                s.set_target(value);
            }
        }
    }
}

impl BiquadFilterNode {
    pub fn new() -> Self {
        Self {
            z1: [0.0, 0.0],
            z2: [0.0, 0.0],
            freq_smooth: None,
        }
    }
}

pub struct BiquadFilterNode {
    z1: [f32; 2],
    z2: [f32; 2],
    freq_smooth: Option<SmoothedValue>,
}

impl DspNode for BiquadFilterNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        ctx: &ProcessContext,
    ) {
        let freq_target = config
            .get("frequency")
            .and_then(|v| v.as_float())
            .unwrap_or(1000.0) as f32;
        let q = config.get("q").and_then(|v| v.as_float()).unwrap_or(0.707) as f32;
        let filter_type = config.get("type").and_then(|v| v.as_string());

        let smooth = self
            .freq_smooth
            .get_or_insert_with(|| SmoothedValue::new(freq_target, ctx.sample_rate, 10.0));
        let freq = smooth.next();

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
            "bandpass" => {
                let b0 = alpha;
                let b1 = 0.0;
                let b2 = -alpha;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                (b0, b1, b2, a0, a1, a2)
            }
            "notch" => {
                let b0 = 1.0;
                let b1 = -2.0 * cos_w0;
                let b2 = 1.0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                (b0, b1, b2, a0, a1, a2)
            }
            "peak" => {
                let gain_db = config
                    .get("gain_db")
                    .and_then(|v| v.as_float())
                    .unwrap_or(0.0) as f32;
                let a_val = 10.0_f32.powf(gain_db / 40.0);
                let b0 = 1.0 + alpha * a_val;
                let b1 = -2.0 * cos_w0;
                let b2 = 1.0 - alpha * a_val;
                let a0 = 1.0 + alpha / a_val;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha / a_val;
                (b0, b1, b2, a0, a1, a2)
            }
            _ => {
                // LPF
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

    fn update_parameter(&mut self, param: &str, value: f32) {
        if param == "frequency" {
            if let Some(s) = &mut self.freq_smooth {
                s.set_target(value);
            }
        }
    }
}

pub struct CompressorNode {
    envelope: f32,
}

impl CompressorNode {
    pub fn new() -> Self {
        Self { envelope: 0.0 }
    }
}

impl DspNode for CompressorNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        ctx: &ProcessContext,
    ) {
        let threshold_db = config
            .get("threshold_db")
            .and_then(|v| v.as_float())
            .unwrap_or(-20.0) as f32;
        let ratio = config
            .get("ratio")
            .and_then(|v| v.as_float())
            .unwrap_or(4.0) as f32;
        let attack_ms = config
            .get("attack_ms")
            .and_then(|v| v.as_float())
            .unwrap_or(10.0) as f32;
        let release_ms = config
            .get("release_ms")
            .and_then(|v| v.as_float())
            .unwrap_or(100.0) as f32;

        let threshold = 10.0_f32.powf(threshold_db / 20.0);
        let attack_alpha = 1.0 - (-1.0 / (attack_ms * ctx.sample_rate / 1000.0)).exp();
        let release_alpha = 1.0 - (-1.0 / (release_ms * ctx.sample_rate / 1000.0)).exp();

        let (l, r) = if inputs.len() >= 2 {
            (inputs[0], inputs[1])
        } else if inputs.len() == 1 {
            (inputs[0], inputs[0])
        } else {
            (0.0, 0.0)
        };

        let peak = l.abs().max(r.abs());
        let alpha = if peak > self.envelope {
            attack_alpha
        } else {
            release_alpha
        };
        self.envelope += alpha * (peak - self.envelope);

        let gain = if self.envelope > threshold {
            let over_db = 20.0 * (self.envelope / threshold).log10();
            let reduction_db = over_db * (1.0 - 1.0 / ratio);
            10.0_f32.powf(-reduction_db / 20.0)
        } else {
            1.0
        };

        outputs[0][0] = l * gain;
        outputs[0][1] = r * gain;
    }
}

pub struct ForeignNode {
    host: Option<PluginHost>,
    plugin_name: String,
    buffer_size: usize,
    in_buffer: Vec<f32>,
    out_buffer: Vec<f32>,
    buffer_idx: usize,
    has_crashed: bool,
}

impl ForeignNode {
    pub fn new(plugin_name: String, buffer_size: usize) -> Self {
        Self {
            host: None,
            plugin_name,
            buffer_size,
            in_buffer: vec![0.0; buffer_size],
            out_buffer: vec![0.0; buffer_size],
            buffer_idx: 0,
            has_crashed: false,
        }
    }

    fn ensure_host(&mut self) -> bool {
        if self.has_crashed {
            return false;
        }
        if self.host.is_some() {
            return true;
        }

        match PluginHost::new(&self.plugin_name, self.buffer_size) {
            Ok(h) => {
                self.host = Some(h);
                true
            }
            Err(_) => {
                self.has_crashed = true;
                false
            }
        }
    }
}

impl DspNode for ForeignNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        _config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
        if !self.ensure_host() {
            // Fallback: Silence or pass through
            outputs[0] = [0.0, 0.0];
            return;
        }

        let input_val = if !inputs.is_empty() { inputs[0] } else { 0.0 };
        self.in_buffer[self.buffer_idx] = input_val;

        // We output the delayed sample from the previous block's processing
        // This introduces 1-block latency, which is expected for out-of-process
        outputs[0][0] = self.out_buffer[self.buffer_idx];
        outputs[0][1] = self.out_buffer[self.buffer_idx];

        self.buffer_idx += 1;
        if self.buffer_idx >= self.buffer_size {
            self.buffer_idx = 0;
            // Process the block
            if let Some(host) = &mut self.host {
                if host.process(&self.in_buffer, &mut self.out_buffer).is_err() {
                    self.has_crashed = true;
                    self.host = None;
                    if let Some(flag) = _ctx.crash_flag {
                        flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                }
            }
        }
    }

    fn update_parameter(&mut self, param: &str, value: f32) {
        if let Some(host) = &mut self.host {
            // Dummy: try to parse param as u32 id
            if let Ok(id) = param.parse::<u32>() {
                let _ = host.set_parameter(id, value);
            }
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
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
        let delay_samples = config
            .get("delay_samples")
            .and_then(|v| v.as_float())
            .unwrap_or(4410.0) as usize;
        let feedback = config
            .get("feedback")
            .and_then(|v| v.as_float())
            .unwrap_or(0.5) as f32;

        let read_pos = (self.write_pos + self.buffer.len() - delay_samples) % self.buffer.len();
        let delayed = self.buffer[read_pos];

        outputs[0][0] = delayed[0];
        outputs[0][1] = delayed[1];

        let in_l = if inputs.len() >= 1 { inputs[0] } else { 0.0 };
        let in_r = if inputs.len() >= 2 { inputs[1] } else { 0.0 };

        self.buffer[self.write_pos] = [in_l + delayed[0] * feedback, in_r + delayed[1] * feedback];

        self.write_pos = (self.write_pos + 1) % self.buffer.len();
    }
}

// ──────────────────────────────────────────────
// §3 — Math
// ──────────────────────────────────────────────

pub struct AddNode;
impl AddNode {
    pub fn new() -> Self {
        Self
    }
}

impl DspNode for AddNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        _config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
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
impl MultiplyNode {
    pub fn new() -> Self {
        Self
    }
}

impl DspNode for MultiplyNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        _config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
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
impl ClipNode {
    pub fn new() -> Self {
        Self
    }
}

impl DspNode for ClipNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
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
impl TriggerNode {
    pub fn new() -> Self {
        Self
    }
}

impl DspNode for TriggerNode {
    fn process(
        &mut self,
        _inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        ctx: &ProcessContext,
    ) {
        let trigger_sample = config
            .get("sample")
            .and_then(|v| v.as_float())
            .unwrap_or(0.0) as u64;
        let val = if ctx.global_sample_index == trigger_sample {
            1.0
        } else {
            0.0
        };
        outputs[0][0] = val;
        outputs[0][1] = val;
    }
}

#[derive(Clone, Copy, PartialEq)]
enum EnvState {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
    FastRelease,
}

pub struct EnvelopeNode {
    state: EnvState,
    level: f32,
}

impl EnvelopeNode {
    pub fn new() -> Self {
        Self {
            state: EnvState::Idle,
            level: 0.0,
        }
    }

    pub fn is_idle(&self) -> bool {
        self.state == EnvState::Idle
    }
}

impl DspNode for EnvelopeNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        ctx: &ProcessContext,
    ) {
        let a = config
            .get("attack")
            .and_then(|v| v.as_float())
            .unwrap_or(0.1) as f32;
        let d = config
            .get("decay")
            .and_then(|v| v.as_float())
            .unwrap_or(0.1) as f32;
        let s = config
            .get("sustain")
            .and_then(|v| v.as_float())
            .unwrap_or(0.5) as f32;
        let r = config
            .get("release")
            .and_then(|v| v.as_float())
            .unwrap_or(0.5) as f32;

        let gate = inputs.get(0).cloned().unwrap_or(0.0) > 0.0;

        match self.state {
            EnvState::Idle => {
                if gate {
                    self.state = EnvState::Attack;
                }
            }
            EnvState::Attack => {
                if !gate {
                    self.state = EnvState::Release;
                } else {
                    self.level += 1.0 / (a * ctx.sample_rate);
                    if self.level >= 1.0 {
                        self.level = 1.0;
                        self.state = EnvState::Decay;
                    }
                }
            }
            EnvState::Decay => {
                if !gate {
                    self.state = EnvState::Release;
                } else {
                    self.level -= (1.0 - s) / (d * ctx.sample_rate);
                    if self.level <= s {
                        self.level = s;
                        self.state = EnvState::Sustain;
                    }
                }
            }
            EnvState::Sustain => {
                if !gate {
                    self.state = EnvState::Release;
                }
            }
            EnvState::Release => {
                if gate {
                    self.state = EnvState::Attack;
                } else {
                    self.level -= s / (r * ctx.sample_rate);
                    if self.level <= 0.0 {
                        self.level = 0.0;
                        self.state = EnvState::Idle;
                    }
                }
            }
            EnvState::FastRelease => {
                // Fade out in 5ms to avoid pops
                let fade_out_rate = 1.0 / (0.005 * ctx.sample_rate);
                self.level -= fade_out_rate;
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.state = EnvState::Idle;
                }
            }
        }

        outputs[0][0] = self.level;
        outputs[0][1] = self.level;
    }

    fn update_parameter(&mut self, param: &str, _value: f32) {
        if param == "steal" {
            self.state = EnvState::FastRelease;
        }
    }

    fn extract_state(&self) -> NodeState {
        NodeState::Envelope {
            state_raw: self.state as u8,
            level: self.level,
        }
    }

    fn inject_state(&mut self, state: &NodeState) {
        if let NodeState::Envelope { state_raw, level } = state {
            self.state = match *state_raw {
                0 => EnvState::Idle,
                1 => EnvState::Attack,
                2 => EnvState::Decay,
                3 => EnvState::Sustain,
                4 => EnvState::Release,
                5 => EnvState::FastRelease,
                _ => EnvState::Idle,
            };
            self.level = *level;
        }
    }
}

pub struct SequencerNode {
    last_step_idx: i32,
}

impl SequencerNode {
    pub fn new() -> Self {
        Self { last_step_idx: -1 }
    }
}

impl DspNode for SequencerNode {
    fn process(
        &mut self,
        _inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        ctx: &ProcessContext,
    ) {
        let bpm = config
            .get("bpm")
            .and_then(|v| v.as_float())
            .unwrap_or(120.0) as f32;
        let steps_data = config.get("steps").and_then(|v| v.as_list());

        let samples_per_step = (60.0 / (bpm * 4.0)) * ctx.sample_rate;
        let current_step_idx = ((ctx.global_sample_index as f32 / samples_per_step) as i32) % 16;

        outputs[0] = [0.0, 0.0];

        if current_step_idx != self.last_step_idx {
            // Step boundary!
            if let Some(steps) = steps_data {
                let step = &steps[current_step_idx as usize];
                if let Some(note_val) = step.as_float() {
                    // Simple protocol: L=1.0 (NoteOn), R=(Note<<8 | Velocity)
                    // For now, velocity is fixed at 100
                    let note = note_val as u32;
                    let vel = 100u32;
                    outputs[0][0] = 1.0; // NoteOn
                    outputs[0][1] = ((note << 8) | vel) as f32;
                } else {
                    // NoteOff if the previous step had a note?
                    // For now, let's just send NoteOff for ALL notes if step is empty
                    // Or more precisely, we need to track what note we started.
                    outputs[0][0] = 2.0; // NoteOff (All or specific)
                }
            }
            self.last_step_idx = current_step_idx;
        }
    }
}

pub struct AutomationNode;
impl AutomationNode {
    pub fn new() -> Self {
        Self
    }
}

impl DspNode for AutomationNode {
    fn process(
        &mut self,
        _inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        ctx: &ProcessContext,
    ) {
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
    fn process(
        &mut self,
        _inputs: &[f32],
        outputs: &mut [[f32; 2]],
        _config: &ConfigSnapshot,
        ctx: &ProcessContext,
    ) {
        // 1. Drain queue into pending
        while let Ok(event) = self.event_rx.try_recv() {
            self.pending_events.push(event);
        }

        // 2. Process events for current sample
        self.pending_events.retain(|event| {
            if event.sample_index <= ctx.global_sample_index {
                let status = event.message[0] & 0xF0;
                match status {
                    0x90 => {
                        // Note On
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
                    0x80 => {
                        // Note Off
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

// ──────────────────────────────────────────────
// §4 — Advanced & Chaos
// ──────────────────────────────────────────────

pub struct WavefolderNode {
    #[allow(dead_code)]
    stages: usize,
}

impl WavefolderNode {
    pub fn new() -> Self {
        Self { stages: 4 }
    }
}

impl DspNode for WavefolderNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
        let gain = config.get("gain").and_then(|v| v.as_float()).unwrap_or(1.0) as f32;
        let stages = config
            .get("stages")
            .and_then(|v| match v {
                dirtydata_core::types::ConfigValue::Int(i) => Some(*i as usize),
                _ => None,
            })
            .unwrap_or(4);

        for i in 0..outputs.len() {
            let mut l = inputs.get(i * 2).cloned().unwrap_or(0.0) * gain;
            let mut r = inputs.get(i * 2 + 1).cloned().unwrap_or(0.0) * gain;

            for _ in 0..stages {
                l = (l * std::f32::consts::PI * 0.5).sin();
                r = (r * std::f32::consts::PI * 0.5).sin();
            }
            outputs[i] = [l, r];
        }
    }
}

pub struct LorenzNode {
    state: [f32; 3],
    sigma: f32,
    rho: f32,
    beta: f32,
}

impl LorenzNode {
    pub fn new() -> Self {
        Self {
            state: [0.1, 0.0, 0.0],
            sigma: 10.0,
            rho: 28.0,
            beta: 8.0 / 3.0,
        }
    }
}

impl DspNode for LorenzNode {
    fn process(
        &mut self,
        _inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        ctx: &ProcessContext,
    ) {
        let speed = config
            .get("speed")
            .and_then(|v| v.as_float())
            .unwrap_or(1.0) as f32;
        let dt = speed / ctx.sample_rate;

        let sigma = self.sigma;
        let rho = self.rho;
        let beta = self.beta;

        rk4_step(&mut self.state, dt, 0.0, |state, _t| {
            let x = state[0];
            let y = state[1];
            let z = state[2];
            vec![sigma * (y - x), x * (rho - z) - y, x * y - beta * z]
        });

        // Output X, Y, Z as 3 mono signals (mapped to stereo ports)
        outputs[0] = [self.state[0] * 0.05, self.state[1] * 0.05];
        outputs[1] = [self.state[2] * 0.05, 0.0];
    }
}

pub struct MackeyGlassNode {
    history: VecDeque<f32>,
    #[allow(dead_code)]
    tau_samples: usize,
    beta: f32,
    gamma: f32,
    n: f32,
    current_x: f32,
}

impl MackeyGlassNode {
    pub fn new(tau_ms: f32, sample_rate: f32) -> Self {
        let tau_samples = (tau_ms * 0.001 * sample_rate) as usize;
        let mut history = VecDeque::with_capacity(tau_samples + 1);
        for _ in 0..=tau_samples {
            history.push_back(0.5);
        }
        Self {
            history,
            tau_samples,
            beta: 2.0,
            gamma: 1.0,
            n: 10.0,
            current_x: 0.5,
        }
    }
}

impl DspNode for MackeyGlassNode {
    fn process(
        &mut self,
        _inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        ctx: &ProcessContext,
    ) {
        let speed = config
            .get("speed")
            .and_then(|v| v.as_float())
            .unwrap_or(1.0) as f32;
        let dt = speed / ctx.sample_rate;

        let x_tau = *self.history.front().unwrap();

        // Simple integration for Mackey-Glass (RK4-ish applied locally)
        let f = |x: f32, xt: f32| self.beta * xt / (1.0 + xt.powf(self.n)) - self.gamma * x;

        let k1 = f(self.current_x, x_tau);
        let k2 = f(self.current_x + k1 * dt * 0.5, x_tau);
        let k3 = f(self.current_x + k2 * dt * 0.5, x_tau);
        let k4 = f(self.current_x + k3 * dt, x_tau);

        self.current_x += (dt / 6.0) * (k1 + 2.0 * k2 + 2.0 * k3 + k4);
        self.history.push_back(self.current_x);
        self.history.pop_front();

        outputs[0] = [self.current_x, self.current_x];
    }
}

pub struct GrayScottNode {
    u: Vec<f32>,
    v: Vec<f32>,
    size: usize,
    f: f32,
    k: f32,
    du: f32,
    dv: f32,
}

impl GrayScottNode {
    pub fn new(size: usize) -> Self {
        let u = vec![1.0; size];
        let mut v = vec![0.0; size];
        // Seed some life
        for i in (size / 2 - 5)..(size / 2 + 5) {
            v[i] = 0.5;
        }
        Self {
            u,
            v,
            size,
            f: 0.0545,
            k: 0.062,
            du: 0.1,
            dv: 0.05,
        }
    }
}

impl DspNode for GrayScottNode {
    fn process(
        &mut self,
        _inputs: &[f32],
        outputs: &mut [[f32; 2]],
        _config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
        let mut next_u = self.u.clone();
        let mut next_v = self.v.clone();

        for i in 0..self.size {
            let prev = if i == 0 { self.size - 1 } else { i - 1 };
            let next = if i == self.size - 1 { 0 } else { i + 1 };

            let lap_u = self.u[prev] + self.u[next] - 2.0 * self.u[i];
            let lap_v = self.v[prev] + self.v[next] - 2.0 * self.v[i];

            let uv2 = self.u[i] * self.v[i] * self.v[i];

            next_u[i] += self.du * lap_u - uv2 + self.f * (1.0 - self.u[i]);
            next_v[i] += self.dv * lap_v + uv2 - (self.f + self.k) * self.v[i];
        }

        self.u = next_u;
        self.v = next_v;

        // Output center point as audio
        outputs[0] = [
            self.u[self.size / 2] * 2.0 - 1.0,
            self.v[self.size / 2] * 2.0 - 1.0,
        ];
    }
}

pub struct SlewLimiterNode {
    current: f32,
}

impl SlewLimiterNode {
    pub fn new() -> Self {
        Self { current: 0.0 }
    }
}

impl DspNode for SlewLimiterNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        ctx: &ProcessContext,
    ) {
        let rise = config.get("rise").and_then(|v| v.as_float()).unwrap_or(0.1) as f32;
        let fall = config.get("fall").and_then(|v| v.as_float()).unwrap_or(0.1) as f32;

        for i in 0..outputs.len() {
            let target = inputs.get(i * 2).cloned().unwrap_or(0.0);
            let diff = target - self.current;
            let limit = if diff > 0.0 { rise } else { fall };
            let step = diff.clamp(-limit / ctx.sample_rate, limit / ctx.sample_rate);
            self.current += step;
            outputs[i] = [self.current, self.current];
        }
    }
}

pub struct SampleHoldNode {
    last_val: [f32; 2],
    last_trig: f32,
}

impl SampleHoldNode {
    pub fn new() -> Self {
        Self {
            last_val: [0.0, 0.0],
            last_trig: 0.0,
        }
    }
}

impl DspNode for SampleHoldNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        _config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
        for i in 0..outputs.len() {
            let sig_l = inputs.get(i * 2).cloned().unwrap_or(0.0);
            let sig_r = inputs.get(i * 2 + 1).cloned().unwrap_or(0.0);
            let trig = inputs.get(i * 2 + 2).cloned().unwrap_or(0.0); // Assume 3rd input is trigger

            if trig > 0.5 && self.last_trig <= 0.5 {
                self.last_val = [sig_l, sig_r];
            }
            self.last_trig = trig;
            outputs[i] = self.last_val;
        }
    }
}

pub struct ClockNode {
    phase: f32,
}

impl ClockNode {
    pub fn new() -> Self {
        Self { phase: 0.0 }
    }
}

impl DspNode for ClockNode {
    fn process(
        &mut self,
        _inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        ctx: &ProcessContext,
    ) {
        let bpm = config
            .get("bpm")
            .and_then(|v| v.as_float())
            .unwrap_or(120.0) as f32;
        let division = config
            .get("division")
            .and_then(|v| v.as_float())
            .unwrap_or(4.0) as f32; // Default 1/4

        let freq = (bpm / 60.0) * (division / 4.0);
        let phase_step = freq / ctx.sample_rate;

        for i in 0..outputs.len() {
            let old_phase = self.phase;
            self.phase = (self.phase + phase_step).fract();

            let trigger = if self.phase < old_phase { 1.0 } else { 0.0 };
            outputs[i] = [trigger, trigger];
        }
    }
}

pub struct ProbabilityGateNode {
    rng: Pcg32,
}

impl ProbabilityGateNode {
    pub fn new() -> Self {
        Self {
            rng: Pcg32::seed_from_u64(42),
        }
    }
}

impl DspNode for ProbabilityGateNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
        let prob = config
            .get("probability")
            .and_then(|v| v.as_float())
            .unwrap_or(0.5) as f32;

        for i in 0..outputs.len() {
            let trig = inputs.get(i * 2).cloned().unwrap_or(0.0);
            let mut out = 0.0;
            if trig > 0.5 {
                if self.rng.random::<f32>() < prob {
                    out = 1.0;
                }
            }
            outputs[i] = [out, out];
        }
    }
}

pub struct ReverbNode {
    delays: Vec<VecDeque<f32>>,
    feedback_matrix: [[f32; 4]; 4],
}

impl ReverbNode {
    pub fn new(sample_rate: f32) -> Self {
        let delay_times = [0.037, 0.043, 0.051, 0.061]; // Primes in seconds
        let delays = delay_times
            .iter()
            .map(|&t| {
                let size = (t * sample_rate) as usize;
                let mut dq = VecDeque::with_capacity(size);
                for _ in 0..size {
                    dq.push_back(0.0);
                }
                dq
            })
            .collect();

        // 4x4 Hadamard matrix for diffusion
        let h = 0.5;
        let feedback_matrix = [[h, h, h, h], [h, -h, h, -h], [h, h, -h, -h], [h, -h, -h, h]];

        Self {
            delays,
            feedback_matrix,
        }
    }
}

impl DspNode for ReverbNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
        let decay = config
            .get("decay")
            .and_then(|v| v.as_float())
            .unwrap_or(0.5) as f32;
        let mix = config.get("mix").and_then(|v| v.as_float()).unwrap_or(0.3) as f32;

        for i in 0..outputs.len() {
            let input_l = inputs.get(i * 2).cloned().unwrap_or(0.0);
            let input_r = inputs.get(i * 2 + 1).cloned().unwrap_or(0.0);
            let mono_in = (input_l + input_r) * 0.5;

            // 1. Read delay outputs
            let mut y = [0.0; 4];
            for j in 0..4 {
                y[j] = *self.delays[j].front().unwrap();
            }

            // 2. Compute feedback
            let mut fb = [0.0; 4];
            for row in 0..4 {
                for col in 0..4 {
                    fb[row] += self.feedback_matrix[row][col] * y[col];
                }
            }

            // 3. Inject input and write back to delays
            for j in 0..4 {
                self.delays[j].push_back(mono_in + fb[j] * decay);
                self.delays[j].pop_front();
            }

            // 4. Output mix (L=Y0+Y1, R=Y2+Y3 for pseudo-stereo)
            let wet_l = y[0] + y[1];
            let wet_r = y[2] + y[3];

            outputs[i] = [
                input_l * (1.0 - mix) + wet_l * mix,
                input_r * (1.0 - mix) + wet_r * mix,
            ];
        }
    }
}

pub struct Grain {
    pos: f32,
    duration_samples: f32,
    current_sample: f32,
    active: bool,
}

pub struct GranularNode {
    buffer: Vec<[f32; 2]>,
    write_pos: usize,
    grains: Vec<Grain>,
    next_grain_samples: f32,
}

impl GranularNode {
    pub fn new(sample_rate: f32) -> Self {
        let buf_size = (sample_rate * 2.0) as usize; // 2 seconds buffer
        let mut grains = Vec::new();
        for _ in 0..16 {
            grains.push(Grain {
                pos: 0.0,
                duration_samples: 0.0,
                current_sample: 0.0,
                active: false,
            });
        }
        Self {
            buffer: vec![[0.0, 0.0]; buf_size],
            write_pos: 0,
            grains,
            next_grain_samples: 0.0,
        }
    }
}

impl DspNode for GranularNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        ctx: &ProcessContext,
    ) {
        let pos_norm = config
            .get("position")
            .and_then(|v| v.as_float())
            .unwrap_or(0.5) as f32;
        let size_ms = config
            .get("size")
            .and_then(|v| v.as_float())
            .unwrap_or(50.0) as f32;
        let density = config
            .get("density")
            .and_then(|v| v.as_float())
            .unwrap_or(0.5) as f32;

        let size_samples = (size_ms * 0.001 * ctx.sample_rate) as f32;

        // 1. Record input
        for i in 0..outputs.len() {
            let in_l = inputs.get(i * 2).cloned().unwrap_or(0.0);
            let in_r = inputs.get(i * 2 + 1).cloned().unwrap_or(0.0);
            self.buffer[self.write_pos] = [in_l, in_r];
            self.write_pos = (self.write_pos + 1) % self.buffer.len();

            // 2. Schedule new grain
            self.next_grain_samples -= 1.0;
            if self.next_grain_samples <= 0.0 {
                if let Some(grain) = self.grains.iter_mut().find(|g| !g.active) {
                    grain.active = true;
                    grain.current_sample = 0.0;
                    grain.duration_samples = size_samples;
                    // Jittered position
                    let jitter = (rand::random::<f32>() - 0.5) * 0.05;
                    grain.pos = (pos_norm + jitter).clamp(0.0, 1.0);
                }
                self.next_grain_samples = (1.0 - density) * size_samples * 0.5 + 100.0;
            }

            // 3. Process grains
            let mut mixed = [0.0, 0.0];
            for grain in self.grains.iter_mut().filter(|g| g.active) {
                let norm_idx = grain.current_sample / grain.duration_samples;

                // Simple triangle window
                let window = 1.0 - (2.0 * norm_idx - 1.0).abs();

                let read_base = (grain.pos * (self.buffer.len() as f32 - 1.0)) as usize;
                let read_idx = (read_base + grain.current_sample as usize) % self.buffer.len();
                let val = self.buffer[read_idx];

                mixed[0] += val[0] * window;
                mixed[1] += val[1] * window;

                grain.current_sample += 1.0;
                if grain.current_sample >= grain.duration_samples {
                    grain.active = false;
                }
            }

            outputs[i] = mixed;
        }
    }
}

pub struct WasmNode {
    instance: Option<wasmtime::Instance>,
    store: Option<wasmtime::Store<()>>,
    process_fn: Option<wasmtime::TypedFunc<(f32, f32), i64>>,
    failed: bool,
}

impl WasmNode {
    pub fn new() -> Self {
        Self {
            instance: None,
            store: None,
            process_fn: None,
            failed: false,
        }
    }

    fn init(&mut self, path: &str) -> anyhow::Result<()> {
        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::from_file(&engine, path)?;
        let mut store = wasmtime::Store::new(&engine, ());
        let instance = wasmtime::Instance::new(&mut store, &module, &[])?;

        let process_fn = instance.get_typed_func::<(f32, f32), i64>(&mut store, "process")?;

        self.instance = Some(instance);
        self.store = Some(store);
        self.process_fn = Some(process_fn);
        Ok(())
    }
}

impl DspNode for WasmNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
        if self.instance.is_none() && !self.failed {
            if let Some(path) = config.get("path").and_then(|v| v.as_string()) {
                if let Err(e) = self.init(path) {
                    eprintln!("Failed to init WasmNode: {}", e);
                    self.failed = true;
                }
            }
        }

        if let (Some(store), Some(f)) = (self.store.as_mut(), self.process_fn.as_mut()) {
            for i in 0..outputs.len() {
                let in_l = inputs.get(i * 2).cloned().unwrap_or(0.0);
                let in_r = inputs.get(i * 2 + 1).cloned().unwrap_or(0.0);

                match f.call(&mut *store, (in_l, in_r)) {
                    Ok(res) => {
                        // Unpack two f32 from i64
                        let out_l = f32::from_bits((res >> 32) as u32);
                        let out_r = f32::from_bits(res as u32);
                        outputs[i] = [out_l, out_r];
                    }
                    Err(_) => {
                        outputs[i] = [in_l, in_r];
                    }
                }
            }
        } else {
            // Bypass
            for i in 0..outputs.len() {
                outputs[i] = [
                    inputs.get(i * 2).cloned().unwrap_or(0.0),
                    inputs.get(i * 2 + 1).cloned().unwrap_or(0.0),
                ];
            }
        }
    }
}

// ──────────────────────────────────────────────
// §7.3 Missing Gaps Implementation
// ──────────────────────────────────────────────

/// Logic operations on signals (Gate/CV logic).
pub struct LogicNode;
impl LogicNode {
    pub fn new() -> Self {
        Self
    }
}
impl DspNode for LogicNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
        let mode = config
            .get("mode")
            .and_then(|v| v.as_string())
            .map(|s| s.as_str())
            .unwrap_or("AND");
        let threshold = config
            .get("threshold")
            .and_then(|v| v.as_float())
            .unwrap_or(0.5) as f32;

        let a = inputs.get(0).cloned().unwrap_or(0.0) > threshold;
        let b = inputs.get(1).cloned().unwrap_or(0.0) > threshold;

        let res = match mode {
            "AND" => a && b,
            "OR" => a || b,
            "XOR" => a ^ b,
            "NOT" => !a,
            _ => a && b,
        };

        let val = if res { 1.0 } else { 0.0 };
        for out in outputs.iter_mut() {
            *out = [val, val];
        }
    }
}

use rustfft::{num_complex::Complex, FftPlanner};

/// Spectral Freeze Node.
pub struct SpectralFreezeNode {
    size: usize,
    buffer: Vec<f32>,
    fft_result: Vec<Complex<f32>>,
    frozen: bool,
    write_pos: usize,
    read_pos: usize,
}

impl SpectralFreezeNode {
    pub fn new(size: usize) -> Self {
        Self {
            size,
            buffer: vec![0.0; size],
            fft_result: vec![Complex::default(); size],
            frozen: false,
            write_pos: 0,
            read_pos: 0,
        }
    }
}

impl DspNode for SpectralFreezeNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
        let freeze = config
            .get("freeze")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let input = inputs.get(0).cloned().unwrap_or(0.0);

        if freeze && !self.frozen {
            // Perform FFT once and freeze
            let mut planner = FftPlanner::new();
            let fft = planner.plan_fft_forward(self.size);
            let mut complex_buf: Vec<Complex<f32>> =
                self.buffer.iter().map(|&x| Complex::new(x, 0.0)).collect();
            fft.process(&mut complex_buf);
            self.fft_result = complex_buf;
            self.frozen = true;

            // Generate time-domain frozen signal via IFFT
            let ifft = planner.plan_fft_inverse(self.size);
            let mut inv_buf = self.fft_result.clone();
            ifft.process(&mut inv_buf);
            for (i, c) in inv_buf.iter().enumerate() {
                self.buffer[i] = c.re / self.size as f32;
            }
        } else if !freeze {
            self.frozen = false;
        }

        // Fill input buffer if not frozen
        if !self.frozen {
            self.buffer[self.write_pos] = input;
            self.write_pos = (self.write_pos + 1) % self.size;
        }

        // Output logic: if frozen, loop the frozen buffer
        let out_val = if self.frozen {
            let v = self.buffer[self.read_pos];
            self.read_pos = (self.read_pos + 1) % self.size;
            v
        } else {
            input
        };

        for out in outputs.iter_mut() {
            *out = [out_val, out_val];
        }
    }
}

/// FFT Convolution Node.
pub struct FFTConvolveNode {
    size: usize,
    input_buffer: Vec<f32>,
    impulse_buffer: Vec<f32>,
    result_buffer: Vec<f32>,
    pos: usize,
}

impl FFTConvolveNode {
    pub fn new(size: usize) -> Self {
        Self {
            size,
            input_buffer: vec![0.0; size],
            impulse_buffer: vec![0.0; size],
            result_buffer: vec![0.0; size],
            pos: 0,
        }
    }
}

impl DspNode for FFTConvolveNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        _config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
        let input = inputs.get(0).cloned().unwrap_or(0.0);
        let impulse = inputs.get(1).cloned().unwrap_or(0.0);

        self.input_buffer[self.pos] = input;
        self.impulse_buffer[self.pos] = impulse;
        self.pos += 1;

        if self.pos >= self.size {
            // Block process
            let mut planner = FftPlanner::new();
            let fft = planner.plan_fft_forward(self.size);

            let mut in_complex: Vec<Complex<f32>> = self
                .input_buffer
                .iter()
                .map(|&x| Complex::new(x, 0.0))
                .collect();
            let mut imp_complex: Vec<Complex<f32>> = self
                .impulse_buffer
                .iter()
                .map(|&x| Complex::new(x, 0.0))
                .collect();

            fft.process(&mut in_complex);
            fft.process(&mut imp_complex);

            // Multiply in frequency domain
            for i in 0..self.size {
                in_complex[i] *= imp_complex[i];
            }

            let ifft = planner.plan_fft_inverse(self.size);
            ifft.process(&mut in_complex);

            for (i, c) in in_complex.iter().enumerate() {
                self.result_buffer[i] = c.re / self.size as f32;
            }
            self.pos = 0;
        }

        let out_val = self.result_buffer[self.pos];

        for out in outputs.iter_mut() {
            *out = [out_val, out_val];
        }
    }
}

/// OSC Output Node.
pub struct OscOutNode {
    last_sent_val: f32,
    threshold: f32,
}

impl OscOutNode {
    pub fn new() -> Self {
        Self {
            last_sent_val: 0.0,
            threshold: 0.001,
        }
    }
}

impl DspNode for OscOutNode {
    fn process(
        &mut self,
        inputs: &[f32],
        _outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        ctx: &ProcessContext,
    ) {
        let addr = config
            .get("address")
            .and_then(|v| v.as_string())
            .map(|s| s.as_str())
            .unwrap_or("/dirtydata/out");
        let val = inputs.get(0).cloned().unwrap_or(0.0);

        // Rate-limited sending: only send if value changed significantly
        if (val - self.last_sent_val).abs() > self.threshold {
            if let Some(tx) = ctx.osc_tx {
                let _ = tx.try_send(OscMessage {
                    addr: addr.to_string(),
                    args: vec![rosc::OscType::Float(val)],
                });
                self.last_sent_val = val;
            }
        }
    }
}

// ──────────────────────────────────────────────
// Phase 5.5 — Feedback Hell
// ──────────────────────────────────────────────

/// A node that provides a 1-sample delay, enabling explicit feedback loops.
/// Use this to break causal cycles in the graph.
pub struct FeedbackNode {
    latch: [f32; 2],
}

impl FeedbackNode {
    pub fn new() -> Self {
        Self { latch: [0.0, 0.0] }
    }
}

impl DspNode for FeedbackNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        _config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
        // Output the value from the PREVIOUS sample
        outputs[0] = self.latch;

        // Capture the CURRENT sample for the next cycle
        if inputs.len() >= 2 {
            self.latch = [inputs[0], inputs[1]];
        }
    }
}

// ──────────────────────────────────────────────
// §7 — Containers (Encapsulation)
// ──────────────────────────────────────────────

pub struct InputProxyNode {
    value: f32,
}
impl InputProxyNode {
    pub fn new() -> Self {
        Self { value: 0.0 }
    }
}
impl DspNode for InputProxyNode {
    fn process(
        &mut self,
        _inputs: &[f32],
        outputs: &mut [[f32; 2]],
        _config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
        outputs[0] = [self.value, self.value];
    }
    fn update_parameter(&mut self, _param: &str, value: f32) {
        self.value = value;
    }
}

pub struct OutputProxyNode;
impl OutputProxyNode {
    pub fn new() -> Self {
        Self
    }
}
impl DspNode for OutputProxyNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        _config: &ConfigSnapshot,
        _ctx: &ProcessContext,
    ) {
        let val = inputs.get(0).cloned().unwrap_or(0.0);
        outputs[0] = [val, val];
    }
}

pub struct SubGraphNode {
    runner: Option<crate::DspRunner>,
    last_graph_hash: String,
}

impl SubGraphNode {
    pub fn new() -> Self {
        Self {
            runner: None,
            last_graph_hash: String::new(),
        }
    }
}

impl DspNode for SubGraphNode {
    fn process(
        &mut self,
        inputs: &[f32],
        outputs: &mut [[f32; 2]],
        config: &ConfigSnapshot,
        ctx: &ProcessContext,
    ) {
        let graph_json = config
            .get("graph_json")
            .and_then(|v| v.as_string())
            .map(|s| s.as_str())
            .unwrap_or("");
        let hash = blake3::hash(graph_json.as_bytes()).to_string();

        if hash != self.last_graph_hash && !graph_json.is_empty() {
            if let Ok(graph) = serde_json::from_str::<dirtydata_core::ir::Graph>(&graph_json) {
                self.runner = Some(crate::DspRunner::new(graph, None, ctx.sample_rate));
                self.last_graph_hash = hash;
            }
        }

        if let Some(runner) = &mut self.runner {
            let mut proxy_ids = Vec::new();
            for (id, n) in &runner.get_graph().nodes {
                if n.kind == dirtydata_core::types::NodeKind::InputProxy {
                    proxy_ids.push(*id);
                }
            }
            for (id, node) in runner.nodes_mut() {
                if proxy_ids.contains(id) {
                    node.update_parameter("value", inputs.get(0).cloned().unwrap_or(0.0));
                }
            }

            let sub_out = runner.process_sample(ctx);
            outputs[0] = sub_out;
        } else {
            for o in outputs {
                *o = [0.0, 0.0];
            }
        }
    }
}
