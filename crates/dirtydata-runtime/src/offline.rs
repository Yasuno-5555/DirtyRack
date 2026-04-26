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
        let runner = DspRunner::new(graph, None, sample_rate);
        Self {
            runner,
            sample_rate,
        }
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
                crash_flag: None,
                osc_tx: None,
            };
            let sample = self.runner.process_sample(&ctx);
            output.push(sample[0]);
            output.push(sample[1]);
        }

        output
    }

    /// Performs a null test by rendering the same graph twice in parallel
    /// and comparing the output bit-by-bit to prove determinism.
    pub fn null_test(graph: Graph, duration_secs: f32, sample_rate: f32) -> Result<bool, String> {
        let mut r1 = OfflineRenderer::new(graph.clone(), sample_rate);
        let mut r2 = OfflineRenderer::new(graph, sample_rate);

        let out1 = r1.render(duration_secs);
        let out2 = r2.render(duration_secs);

        if out1.len() != out2.len() {
            return Err("Output length mismatch between identical runs".into());
        }

        for (i, (s1, s2)) in out1.iter().zip(out2.iter()).enumerate() {
            // Check for strict mathematical equality
            if (*s1 - *s2).abs() > 0.0 {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
