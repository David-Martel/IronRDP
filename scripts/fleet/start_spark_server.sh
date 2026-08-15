#!/usr/bin/env bash
# Start a pre-provisioned IronRDP systemd user service over verified SSH.
# Usage: ./start_spark_server.sh <SSH_DESTINATION> <PORT>
#
# The remote ironrdp-server@.service must supply its TLS identity and runtime
# credential through the host's service manager. This launcher never transfers
# or persists a password or private key.

set -euo pipefail

SSH_DESTINATION="${1:?usage: start_spark_server.sh <SSH_DESTINATION> <PORT>}"
TARGET_PORT="${2:?usage: start_spark_server.sh <SSH_DESTINATION> <PORT>}"

if [[ ! "${TARGET_PORT}" =~ ^[0-9]+$ ]] || ((TARGET_PORT < 1 || TARGET_PORT > 65535)); then
    echo "error: port must be an integer from 1 through 65535" >&2
    exit 2
fi

SERVICE_NAME="ironrdp-server@${TARGET_PORT}.service"
SSH_OPTS=(-o BatchMode=yes -o StrictHostKeyChecking=yes)

echo "=== IronRDP Fleet Server Auto-Start ==="
echo "SSH destination: ${SSH_DESTINATION}"
echo "Remote service: ${SERVICE_NAME}"

if ssh "${SSH_OPTS[@]}" -- "${SSH_DESTINATION}" systemctl --user is-active --quiet "${SERVICE_NAME}"; then
    echo "[OK] ${SERVICE_NAME} is already active."
    exit 0
fi

echo "[INFO] Starting ${SERVICE_NAME} through the remote user service manager..."
ssh "${SSH_OPTS[@]}" -- "${SSH_DESTINATION}" systemctl --user start "${SERVICE_NAME}"
ssh "${SSH_OPTS[@]}" -- "${SSH_DESTINATION}" systemctl --user is-active --quiet "${SERVICE_NAME}"

echo "[OK] ${SERVICE_NAME} is active."
