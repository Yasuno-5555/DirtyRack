use bytemuck;
use std::io::{Read, Write};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let plugin_name = args.get(1).map(|s| s.as_str()).unwrap_or("unknown");

    let mut stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    // 2 channels * block size (e.g., 256) = 512 samples -> 2048 bytes
    let mut buf = vec![0u8; 2048];

    // Dummy state for simulating a bug
    let mut call_count = 0;

    loop {
        match stdin.read_exact(&mut buf) {
            Ok(_) => {
                call_count += 1;
                
                // Convert bytes to f32 slice properly handling alignment
                let mut float_buf = vec![0.0f32; buf.len() / 4];
                bytemuck::cast_slice_mut(&mut float_buf).copy_from_slice(&buf);

                // Process (dummy delay/gain)
                for sample in float_buf.iter_mut() {
                    *sample *= 0.5; // Gain reduction
                }

                // Simulate chaos in "vst_buggy" plugin
                if plugin_name == "vst_buggy" && call_count > 100 {
                    // Random crash!
                    std::process::exit(1);
                }

                if plugin_name == "vst_nan" && call_count > 50 {
                    // NaN storm!
                    for sample in float_buf.iter_mut() {
                        *sample = std::f32::NAN;
                    }
                }

                // Write back
                let out_bytes = bytemuck::cast_slice(&float_buf);
                if stdout.write_all(out_bytes).is_err() {
                    break;
                }
                if stdout.flush().is_err() {
                    break;
                }
            }
            Err(_) => {
                // Parent closed pipe
                break;
            }
        }
    }
}
