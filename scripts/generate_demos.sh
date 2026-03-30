#!/bin/bash
# scripts/generate_demos.sh

set -e

# Ensure we have the latest binary
echo "Building bart in release mode..."
cargo build --release

# Ensure target directories exist
mkdir -p assets/vhs

# Function to generate a demo
generate() {
  local tape=$1
  local gif=$2
  echo "Generating $gif from $tape..."
  vhs < "$tape"
}

# Generate all demos
generate assets/vhs/scan_core.tape assets/vhs/scan_core.gif
generate assets/vhs/differential.tape assets/vhs/differential.gif
generate assets/vhs/interactive_tui.tape assets/vhs/interactive_tui.gif
generate assets/vhs/daemon_observability.tape assets/vhs/daemon_observability.gif

echo "Done! All demos generated in assets/vhs/"
