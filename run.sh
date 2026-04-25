#!/bin/bash
# DirtyData — The Creative Operating System

# Check if project is initialized
if [ ! -d ".dirtydata" ]; then
    echo "▶ Initializing new DirtyData project..."
    cargo run -p dirtydata-cli -- init
fi

# Launch the Workbench
echo "▶ Launching DirtyData Workbench..."
cargo run -p dirtydata-cli -- gui
