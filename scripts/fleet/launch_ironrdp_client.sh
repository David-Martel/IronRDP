#!/usr/bin/env bash
# Universal IronRDP Client Launcher Script
set -euo pipefail

REPO_DIR="/home/damartel/dev/repos/IronRDP"
cd "${REPO_DIR}"

if [ ! -f "target/debug/ironrdp-client" ]; then
    echo "[BUILD] Compiling ironrdp-client..."
    cargo build --package ironrdp-client
fi

echo "=== Launching IronRDP GUI Connection Manager ==="
exec ./target/debug/ironrdp-client "$@"
