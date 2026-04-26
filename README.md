# DirtyRack

**Deterministic Eurorack Simulator & Modular DSP Engine**

DirtyRack is a **high-precision deterministic Eurorack simulator** that merges bit-perfect reproducibility with physical modular interaction. It is designed for artists and engineers who love the "chaos" and "fluctuations" of analog but demand digital "Accountability."

More than just a synthesizer, it is a **Forensic Audio Engine** that completely freezes the entire state of a patch, performance gestures, and the flow of time, making them verifiable via hash values.

## Core Design Philosophy

*   **Deterministic Chaos**: Handles chaos attractors and non-linear feedback while guaranteeing 1-bit accuracy for the same seed and input.
*   **Massive Polyphony (16ch)**: Native support for VCV Rack-compatible 16-channel polyphonic cables. A single connection yields orchestral density.
*   **Audio Sanctity**: The audio processing thread is 100% lock-free. It physically prevents glitches caused by UI load or file I/O.
*   **Forensic Observation**: A forensic layer that proves "why this sound happened." The Drift Inspector allows real-time dissection of internal thermal states and individual component variances.
*   **Open Ecology**: Third parties can develop and distribute their own deterministic modules in Rust using the `dirtyrack-sdk`.

## Key Features

1.  **Massive Polyphonic DSP**: All modules support independent 16-voice processing. Complete polyphonic expression with a single cable.
2.  **Analog Imperfection Layer**: Deterministically reproduced "Equipment Personality" and "Thermal Drift." Scientifically emulates the instability unique to analog.
3.  **Aging Knob**: Control everything from "factory-new" shine to 20 years of "vintage decay" with a single global knob.
4.  **Forensic Inspector**: Deeply analyze the internal state of modules. Visualize the causes of pitch drift or filter saturation as objective data.
5.  **Triple-Buffer Visuals**: Maintains the sanctity of the audio thread while delivering smooth 60fps+ waveform projections and LED level displays.
6.  **MIDI-CV Bridge**: Converts external MIDI signals into polyphonic 1V/Oct signals, vanishing the boundary between hardware and software.
7.  **Deterministic Auditing (New)**: Features a Divergence Map to detect "reality splits" at sample precision, and an Intent-to-Sound Trace to track the causality of sound.

### Distribution & Formats

- **Standalone App**: Provides a `.app` bundle for macOS. Ready to use via drag-and-drop to `/Applications`.
- **VST3 / CLAP Plugin**: Operates as a 16ch polyphonic modular inside DAWs (Ableton, Bitwig, Reaper, etc.).
- **CLI Tool**: A command-line interface for deterministic offline rendering and hash verification.

## Quick Start

### Standalone (macOS)
```bash
# Copy DirtyRack.app from the root to /Applications
open ./DirtyRack.app
```

### Plugin Deployment
Place the generated `DirtyRack.clap` from the root into your plugin folder.

**To use as VST3**:
1. Create a directory structure: `DirtyRack.vst3/Contents/MacOS/`.
2. Copy and rename `DirtyRack.clap` to `DirtyRack` inside that directory.

## Project Structure

```text
DirtyRack/
├── crates/
│   ├── dirtyrack-sdk/      # SDK for third-party developers. Core traits and SIMD utilities.
│   ├── dirtyrack-modules/  # Deterministic DSP module arsenal (VCO, VCF, Chaos, etc.)
│   ├── dirtyrack-gui/      # egui-based "Projector." Triple-Buffer synchronization.
│   └── dirtyrack-core/     # Deterministic foundation. DAG engine managing causality.
├── docs/                   # SDK documentation, architecture, philosophy.
└── modules/                # Directory for third-party dynamic libraries (.so, .dll).
```

## Documentation

- [Design Philosophy](docs/design_philosophy.md)
- [Architecture](docs/architecture.md)
- [SDK Documentation](docs/SDK_Documentation.md)
- [Creating Your First Module (Tutorial)](docs/Tutorial_Creating_Modules.md)
- [Japanese README (日本語版)](docs/README_JP.md)

## License

MIT License
