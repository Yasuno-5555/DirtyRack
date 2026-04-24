use arc_swap::ArcSwap;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use dirtydata_core::actions;
use dirtydata_core::ir::Graph;
use dirtydata_core::types::ConfigValue;
use dirtydata_host::PluginHost;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct AudioEngine {
    graph: Arc<ArcSwap<Graph>>,
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

        let graph_swap = Arc::new(ArcSwap::from_pointee(initial_graph));
        let graph_clone = graph_swap.clone();
        
        let crash_flag = Arc::new(AtomicBool::new(false));
        let crash_flag_clone = crash_flag.clone();

        let mut phase: f32 = 0.0;
        let freq: f32 = 440.0; // 440Hz A4

        let err_fn = |err| eprintln!("an error occurred on stream: {}", err);

        // Pre-spawn a worker for the MVP demo
        // (In a real system, we'd spawn when graph changes, but outside the audio thread)
        let mut host_worker = PluginHost::new("vst_nan", 512).ok();
        let mut host_fallback_active = false;
        let mut processing_buffer = vec![0.0; 512]; // reasonable max block size

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let current_graph = graph_clone.load();
                    
                    // Simple MVP DSP interpretation of Graph
                    let mut total_gain_db = 0.0;
                    let mut has_source = false;
                    let mut has_sink = false;
                    let mut has_foreign = false;

                    for node in current_graph.nodes.values() {
                        let name = actions::node_name(node);
                        if name == "Sine" || node.kind == dirtydata_core::types::NodeKind::Source {
                            has_source = true;
                        }
                        if node.kind == dirtydata_core::types::NodeKind::Sink {
                            has_sink = true;
                        }
                        if let dirtydata_core::types::NodeKind::Foreign(_) = &node.kind {
                            has_foreign = true;
                        }
                        if node.kind == dirtydata_core::types::NodeKind::Processor {
                            if let Some(ConfigValue::Float(g)) = node.config.get("gain_db") {
                                total_gain_db += *g as f32;
                            }
                            if let Some(ConfigValue::Float(g)) = node.config.get("band_2_gain") {
                                total_gain_db += *g as f32;
                            }
                        }
                    }

                    let linear_gain = 10.0_f32.powf(total_gain_db / 20.0);
                    let phase_inc = freq * 2.0 * std::f32::consts::PI / sample_rate;

                    // Ensure buffer size is adequate
                    let block_len = data.len();
                    if processing_buffer.len() < block_len {
                        processing_buffer.resize(block_len, 0.0);
                    }

                    // 1. Generate local graph audio
                    for frame in processing_buffer[..block_len].chunks_mut(channels) {
                        let mut sample = 0.0;
                        if has_source && has_sink {
                            sample = phase.sin() * 0.1 * linear_gain;
                            phase = (phase + phase_inc) % (2.0 * std::f32::consts::PI);
                        }
                        for output in frame.iter_mut() {
                            *output = sample;
                        }
                    }

                    // 2. Process Foreign Sandbox Boundary
                    if has_foreign && !host_fallback_active {
                        if let Some(host) = &mut host_worker {
                            let input_copy = processing_buffer[..block_len].to_vec();
                            if let Err(_e) = host.process(&input_copy, &mut processing_buffer[..block_len]) {
                                crash_flag_clone.store(true, Ordering::SeqCst);
                                host_fallback_active = true;
                                // Mute the buffer (fallback)
                                processing_buffer[..block_len].fill(0.0);
                            }
                        }
                    } else if has_foreign && host_fallback_active {
                        // Already crashed, output frozen asset (silence for now)
                        processing_buffer[..block_len].fill(0.0);
                    }

                    // 3. Write out
                    data.copy_from_slice(&processing_buffer[..block_len]);
                },
                err_fn,
                None,
            )?,
            _ => return Err("Unsupported sample format".into()),
        };

        stream.play()?;

        Ok(Self {
            graph: graph_swap,
            crash_flag,
            _stream: stream,
        })
    }

    pub fn check_crash(&self) -> bool {
        self.crash_flag.swap(false, Ordering::SeqCst)
    }

    pub fn update_graph(&self, new_graph: Graph) {
        self.graph.store(Arc::new(new_graph));
    }
}
