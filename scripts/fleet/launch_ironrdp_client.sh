#!/usr/bin/env bash
# Universal IronRDP Client Launcher Script
set -euo pipefail

REPO_DIR="/home/damartel/dev/repos/IronRDP"

echo "=== Launching IronRDP GUI Connection Manager ==="
cd "${REPO_DIR}"
cargo run --package ironrdp-client -- --help
