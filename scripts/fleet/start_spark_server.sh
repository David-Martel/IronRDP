#!/usr/bin/env bash
# Parameterized Fleet Auto-Start Script for Remote IronRDP Servers.
# Usage: ./start_spark_server.sh [HOST_IP] [PORT]

set -euo pipefail

TARGET_HOST="${1:-100.64.0.4}"
TARGET_PORT="${2:-33898}"
SSH_USER="damartel"
SSH_KEY="${HOME}/.ssh/id_ed25519"
SSH_OPTS="-i ${SSH_KEY} -o IdentitiesOnly=yes -o StrictHostKeyChecking=no"

echo "=== IronRDP Fleet Server Auto-Start ==="
echo "Target Host: ${TARGET_HOST}:${TARGET_PORT}"

# Probe target host port status over Tailscale
if nc -z -w 3 "${TARGET_HOST}" "${TARGET_PORT}" 2>/dev/null; then
    echo "[OK] IronRDP server is already running and listening on ${TARGET_HOST}:${TARGET_PORT}."
    exit 0
fi

echo "[INFO] Server is not listening on ${TARGET_HOST}:${TARGET_PORT}. Initiating remote start over SSH..."

ssh ${SSH_OPTS} "${SSH_USER}@${TARGET_HOST}" "bash -s" << 'EOF'
set -e
if ! os_path=\$(which openssl 2>/dev/null); then
    echo "OpenSSL required"
fi

if [ ! -f /tmp/spark_server.crt ]; then
    openssl req -x509 -newkey rsa:2048 -keyout /tmp/spark_server.key -out /tmp/spark_server.crt -days 365 -nodes -subj "/CN=${TARGET_HOST}" >/dev/null 2>&1
fi

fuser -k ${TARGET_PORT}/tcp || true
nohup /home/damartel/dev/repos/IronRDP/target/debug/examples/server --bind-addr 0.0.0.0:${TARGET_PORT} --cert /tmp/spark_server.crt --key /tmp/spark_server.key --sec hybrid --user user --pass pass > /tmp/ironrdp_server_${TARGET_PORT}.log 2>&1 &
sleep 1
EOF

echo "[OK] IronRDP server successfully initiated on ${TARGET_HOST}:${TARGET_PORT}."
