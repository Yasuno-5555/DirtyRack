use crate::nodes::ProcessContext;
use crate::DspRunner;
use dirtydata_core::ir::Graph;

/// A renderer for offline (faster than real-time) audio generation.
pub struct OfflineRenderer {
    runner: DspRunner,
    sample_rate: f32,
}

impl OfflineRenderer {
    pub fn new(graph: Graph, sample_rate: f32) -> Self {
        // Offline rendering currently doesn't support live MIDI input
        let runner = DspRunner::new(graph, None);
        Self { runner, sample_rate }
    }

    /// Renders the specified duration of audio.
    /// Returns interleaved stereo samples [L, R, L, R, ...].
    pub fn render(&mut self, duration_secs: f32) -> Vec<f32> {
        let num_samples = (duration_secs * self.sample_rate) as usize;
        let mut output = Vec::with_capacity(num_samples * 2);

        for i in 0..num_samples {
            let ctx = ProcessContext {
                sample_rate: self.sample_rate,
                global_sample_index: i as u64,
            };
            let sample = self.runner.process_sample(&ctx);
            output.push(sample[0]);
            output.push(sample[1]);
        }

        output
    }
}
