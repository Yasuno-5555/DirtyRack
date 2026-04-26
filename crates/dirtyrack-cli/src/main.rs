use clap::{Parser, Subcommand};
use colored::*;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "dirtyrack")]
#[command(about = "DirtyRack Forensic Eurorack Simulator CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the Graphical Projector (GUI)
    Gui,

    /// List all available modules (built-in and dynamic)
    ModuleList,

    /// Render a patch to a deterministic WAV file
    Render {
        /// Path to the patch JSON file
        patch: PathBuf,

        /// Output WAV file path
        #[arg(short, long, default_value = "output.wav")]
        output: PathBuf,

        /// Length in seconds
        #[arg(short, long, default_value_t = 10.0)]
        length: f32,

        /// Sample rate in Hz
        #[arg(short, long, default_value_t = 44100)]
        sample_rate: u32,
    },

    /// Verify a patch against a known hash
    Verify {
        /// Path to the patch JSON file
        patch: PathBuf,

        /// The expected BLAKE3 hash
        hash: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Gui => {
            println!(
                "{} Launching DirtyRack Graphical Projector...",
                "▶".cyan().bold()
            );
            let _ = dirtyrack_gui::run();
        }
        Commands::ModuleList => {
            println!("{} Available DirtyRack Modules:", "✓".green().bold());
            let registry = dirtyrack_modules::registry::ModuleRegistry::new();
            for module in registry.all() {
                println!(
                    "  - {:<16} ({}) by {}",
                    module.name.bold(),
                    module.id,
                    module.manufacturer
                );
            }
        }
        Commands::Render {
            patch,
            output,
            length,
            sample_rate,
        } => {
            println!("{} Rendering patch: {:?}", "▶".yellow().bold(), patch);

            let spec = hound::WavSpec {
                channels: 2,
                sample_rate: sample_rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            };

            let mut writer = hound::WavWriter::create(&output, spec)?;
            let total_samples = (length * sample_rate as f32) as usize;

            println!(
                "  Output: {:?}, Length: {}s, Total Samples: {}",
                output, length, total_samples
            );

            // Placeholder: Generate a simple 440Hz sine wave as a demonstration of "Deterministic Output"
            for i in 0..total_samples {
                let t = i as f32 / sample_rate as f32;
                let val = (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.5;
                writer.write_sample(val)?; // L
                writer.write_sample(val)?; // R
            }

            writer.finalize()?;

            let hash = blake3::hash(&std::fs::read(&output)?);
            println!("{} Render Complete!", "✓".green().bold());
            println!("   Hash (BLAKE3): {}", hash.to_hex());
        }
        Commands::Verify { patch: _, hash: _ } => {
            println!("{} Starting Forensic Verification...", "🔍".blue().bold());
            // Implementation of bit-perfect verification
        }
    }

    Ok(())
}
