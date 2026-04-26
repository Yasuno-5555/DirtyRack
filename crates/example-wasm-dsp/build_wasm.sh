#!/bin/bash
set -e
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/example_wasm_dsp.wasm ../../example_dsp.wasm
echo "Wasm DSP built to ../../example_dsp.wasm"
