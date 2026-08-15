#!/usr/bin/env bash
# Launch the client from this checkout without a machine-specific repository path.
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "${SCRIPT_DIR}/../.." && pwd)"
cd -- "${REPO_DIR}"

echo "=== Launching IronRDP client ==="
exec cargo run --quiet --package ironrdp-client -- "$@"
