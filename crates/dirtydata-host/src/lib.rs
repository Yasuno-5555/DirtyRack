use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Plugin crashed or closed connection")]
    Crashed,
    #[error("Plugin produced NaN (NaN storm)")]
    NanStorm,
}

pub struct PluginHost {
    child: Child,
    fallback_buffer: Vec<f32>,
}

impl PluginHost {
    pub fn new(plugin_name: &str, buffer_size: usize) -> Result<Self, HostError> {
        // For MVP, we run a dummy worker binary that we will build alongside.
        // We assume it's in the same target dir or path. 
        // For testing, we just invoke `dirtydata-plugin-worker`.
        
        let exe = std::env::current_exe().unwrap_or_default();
        let dir = exe.parent().unwrap_or(std::path::Path::new("."));
        let worker_path = dir.join("dirtydata-plugin-worker");

        let child = Command::new(&worker_path)
            .arg(plugin_name)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        Ok(Self {
            child,
            fallback_buffer: vec![0.0; buffer_size],
        })
    }

    /// Process a block of audio. 
    /// If the plugin crashes or produces NaN, returns HostError so parent can fallback.
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), HostError> {
        let mut stdin = self.child.stdin.as_ref().ok_or(HostError::Crashed)?;
        let mut stdout = self.child.stdout.as_mut().ok_or(HostError::Crashed)?;

        // Write input buffer as bytes
        let in_bytes = bytemuck::cast_slice(input);
        if stdin.write_all(in_bytes).is_err() {
            return Err(HostError::Crashed);
        }
        if stdin.flush().is_err() {
            return Err(HostError::Crashed);
        }

        // Read output buffer as bytes
        let out_bytes = bytemuck::cast_slice_mut(output);
        if stdout.read_exact(out_bytes).is_err() {
            return Err(HostError::Crashed);
        }

        // Check for NaN Storm
        for sample in output.iter() {
            if sample.is_nan() {
                return Err(HostError::NanStorm);
            }
        }

        // Update fallback buffer to latest valid output
        if self.fallback_buffer.len() != output.len() {
            self.fallback_buffer.resize(output.len(), 0.0);
        }
        self.fallback_buffer.copy_from_slice(output);

        Ok(())
    }

    pub fn get_fallback(&self) -> &[f32] {
        &self.fallback_buffer
    }
}
