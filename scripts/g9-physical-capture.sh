#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -lt 6 ] || [ "$#" -gt 7 ]; then
  cat >&2 <<'EOF'
usage:
  g9-physical-capture.sh <peer_ip:port> <node_id> <64_hex_psk> <hostname> <https_ip:port> <https_server_name> [evidence_dir]

The exact-path HTTPS probe target must be a literal IP:port plus TLS server name.
EOF
  exit 2
fi

PEER_ADDR="$1"
NODE_ID="$2"
PSK="$3"
HOSTNAME="$4"
HTTPS_ADDR="$5"
HTTPS_NAME="$6"
OUT_DIR="${7:-g9-evidence-$(date -u +%Y%m%dT%H%M%SZ)}"

mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"

cargo build --release -p host-probe-cli -p peer-egress-cli

export SP3_HTTPS_PROBE_ADDR="$HTTPS_ADDR"
export SP3_HTTPS_PROBE_NAME="$HTTPS_NAME"
export SP3_HTTPS_PROBE_PATH="/"
export SP3_HTTPS_PROBE_MAX_BYTES="1024"

echo "=== exact-path host probe ==="
./target/release/host-probe-cli | tee "$OUT_DIR/direct-path.log"

if ! grep -q "MEASURED_PATHS none_verified_by_tiny_https" "$OUT_DIR/direct-path.log"; then
  echo "G9 physical court requires no verified direct tiny-HTTPS Internet path." >&2
  echo "The direct-path probe found at least one verified path; refusing false PASS." >&2
  exit 3
fi

echo "=== authenticated peer rescue ==="
./target/release/peer-egress-cli client "$PEER_ADDR" "$NODE_ID" "$PSK" "$HOSTNAME"   | tee "$OUT_DIR/peer-rescue.log"

grep -q "authenticated egress peer node_id=" "$OUT_DIR/peer-rescue.log"
grep -q "resolved $HOSTNAME through peer:" "$OUT_DIR/peer-rescue.log"

COMMIT="$(git rev-parse HEAD)"
UTC="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
DIRECT_SHA="$(sha256sum "$OUT_DIR/direct-path.log" | awk '{print $1}')"
PEER_SHA="$(sha256sum "$OUT_DIR/peer-rescue.log" | awk '{print $1}')"

cat > "$OUT_DIR/evidence.txt" <<EOF
gate=G9
result=PASS_CANDIDATE
timestamp_utc=$UTC
git_commit=$COMMIT
platform=linux
kernel=$(uname -srmo)
hostname=$(hostname)
default_path_tiny_https=NO_VERIFIED_PATH
peer_addr=$PEER_ADDR
local_node_id=$NODE_ID
remote_operation=resolve:$HOSTNAME
direct_log_sha256=$DIRECT_SHA
peer_log_sha256=$PEER_SHA
note=Candidate physical evidence; review topology and freshness before marking G9 closed.
EOF

echo "G9_CAPTURE_PASS evidence_dir=$OUT_DIR"
echo "Review evidence.txt plus both raw logs before checking the physical G9 gate."
