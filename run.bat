@echo off
REM DirtyData — The Creative Operating System

REM Check if project is initialized
if not exist .dirtydata (
    echo [▶] Initializing new DirtyData project...
    cargo run -p dirtydata-cli -- init
)

REM Launch the Workbench
echo [▶] Launching DirtyData Workbench...
cargo run -p dirtydata-cli -- gui
