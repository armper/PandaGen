#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "[runpanda] Building fresh ISO..."
cargo xtask iso

echo "[runpanda] Launching QEMU..."
cargo xtask qemu "$@"
