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

#[repr(u8)]
#[derive(Debug, Clone, Copy)]
pub enum HostCommand {
    Process = 0,
    SetParameter = 1,
    GetState = 2,
    SetState = 3,
}

pub struct PluginHost {
    child: Child,
    fallback_buffer: Vec<f32>,
}

impl PluginHost {
    pub fn new(plugin_name: &str, buffer_size: usize) -> Result<Self, HostError> {
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

    pub fn set_parameter(&mut self, param_id: u32, value: f32) -> Result<(), HostError> {
        let mut stdin = self.child.stdin.as_ref().ok_or(HostError::Crashed)?;

        let cmd = HostCommand::SetParameter as u8;
        stdin.write_all(&[cmd])?;
        stdin.write_all(&param_id.to_le_bytes())?;
        stdin.write_all(&value.to_le_bytes())?;
        stdin.flush()?;

        Ok(())
    }

    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), HostError> {
        let mut stdin = self.child.stdin.as_ref().ok_or(HostError::Crashed)?;
        let mut stdout = self.child.stdout.as_mut().ok_or(HostError::Crashed)?;

        // Send Command
        let cmd = HostCommand::Process as u8;
        stdin.write_all(&[cmd])?;

        // Send size (u32)
        let size = (input.len() as u32);
        stdin.write_all(&size.to_le_bytes())?;

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

        // Update fallback buffer
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
