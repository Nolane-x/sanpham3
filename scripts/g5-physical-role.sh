#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage:
  SP3_PEER_PSK_HEX=<64_hex_psk> scripts/g5-physical-role.sh egress <bind_addr> <node_id> <expected_relay_node> [evidence_dir]
  SP3_PEER_PSK_HEX=<64_hex_psk> scripts/g5-physical-role.sh relay <bind_addr> <node_id> <upstream_ip:port> <expected_upstream_node> <expected_downstream_node> [evidence_dir]
  SP3_PEER_PSK_HEX=<64_hex_psk> scripts/g5-physical-role.sh client <relay_ip:port> <node_id> <expected_relay_node> <hostname> [evidence_dir]

Run the roles on three different machines/devices.
EOF
}

require_psk() {
  : "${SP3_PEER_PSK_HEX:?SP3_PEER_PSK_HEX must contain the shared 64-hex PSK}"
  if [[ ! "$SP3_PEER_PSK_HEX" =~ ^[0-9a-fA-F]{64}$ ]]; then
    echo "SP3_PEER_PSK_HEX must be exactly 64 hexadecimal characters" >&2
    exit 2
  fi
}

safe_name() {
  printf '%s' "$1" | tr -c 'A-Za-z0-9._-' '_'
}

prepare_evidence() {
  local role="$1"
  local requested="${2:-}"
  local stamp
  stamp="$(date -u +%Y%m%dT%H%M%SZ)"
  if [[ -n "$requested" ]]; then
    EVIDENCE_DIR="$requested"
  else
    EVIDENCE_DIR="evidence/g5-${role}-${stamp}"
  fi
  mkdir -p "$EVIDENCE_DIR"
  LOG_FILE="$EVIDENCE_DIR/${role}.log"
  META_FILE="$EVIDENCE_DIR/metadata.txt"
}

build_binary() {
  cargo build --release -p peer-egress-cli
  BIN="./target/release/peer-egress-cli"
  if [[ ! -x "$BIN" ]]; then
    echo "peer-egress-cli binary not found at $BIN" >&2
    exit 2
  fi
}

write_metadata() {
  local role="$1"
  shift
  {
    echo "timestamp_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "git_commit=$(git rev-parse HEAD)"
    echo "role=$role"
    echo "hostname=$(hostname 2>/dev/null || true)"
    echo "os=$(uname -a 2>/dev/null || true)"
    echo "binary=$BIN"
    echo "binary_sha256=$(sha256sum "$BIN" | awk '{print $1}')"
    for item in "$@"; do
      echo "$item"
    done
  } >"$META_FILE"
}

run_logged() {
  set +e
  "$@" 2>&1 | tee "$LOG_FILE"
  local status=${PIPESTATUS[0]}
  set -e
  return "$status"
}

finalize_hashes() {
  (
    cd "$EVIDENCE_DIR"
    sha256sum "$(basename "$LOG_FILE")" "$(basename "$META_FILE")" >SHA256SUMS
  )
}

run_egress() {
  [[ $# -eq 4 || $# -eq 5 ]] || { usage; exit 2; }
  local bind_addr="$2"
  local node_id="$3"
  local expected_relay="$4"
  local evidence="${5:-}"

  prepare_evidence "egress" "$evidence"
  write_metadata "egress"     "bind_addr=$bind_addr"     "node_id=$node_id"     "expected_relay_node=$expected_relay"

  run_logged "$BIN" server "$bind_addr" "$node_id" -
  grep -q "authenticated peer node_id=$expected_relay" "$LOG_FILE"
  grep -q "request served" "$LOG_FILE"
  echo "G5_ROLE_PASS role=egress node_id=$node_id relay_node=$expected_relay"
  finalize_hashes
}

run_relay() {
  [[ $# -eq 6 || $# -eq 7 ]] || { usage; exit 2; }
  local bind_addr="$2"
  local node_id="$3"
  local upstream="$4"
  local expected_upstream="$5"
  local expected_downstream="$6"
  local evidence="${7:-}"

  prepare_evidence "relay" "$evidence"
  write_metadata "relay"     "bind_addr=$bind_addr"     "node_id=$node_id"     "upstream_addr=$upstream"     "expected_upstream_node=$expected_upstream"     "expected_downstream_node=$expected_downstream"

  run_logged "$BIN" relay-server "$bind_addr" "$node_id" - "$upstream"
  grep -q "authenticated upstream node_id=$expected_upstream" "$LOG_FILE"
  grep -q "authenticated downstream node_id=$expected_downstream" "$LOG_FILE"
  grep -q "relayed one request" "$LOG_FILE"
  echo "G5_ROLE_PASS role=relay node_id=$node_id downstream_node=$expected_downstream upstream_node=$expected_upstream"
  finalize_hashes
}

run_client() {
  [[ $# -eq 5 || $# -eq 6 ]] || { usage; exit 2; }
  local relay_addr="$2"
  local node_id="$3"
  local expected_relay="$4"
  local target_hostname="$5"
  local evidence="${6:-}"

  prepare_evidence "client" "$evidence"
  write_metadata "client"     "relay_addr=$relay_addr"     "node_id=$node_id"     "expected_relay_node=$expected_relay"     "target_hostname=$target_hostname"

  run_logged "$BIN" client "$relay_addr" "$node_id" - "$target_hostname"
  grep -q "authenticated egress peer node_id=$expected_relay" "$LOG_FILE"
  grep -q "resolved $target_hostname through peer:" "$LOG_FILE"
  echo "G5_ROLE_PASS role=client node_id=$node_id relay_node=$expected_relay hostname=$(safe_name "$target_hostname")"
  finalize_hashes
}

main() {
  [[ $# -ge 1 ]] || { usage; exit 2; }
  require_psk
  build_binary

  case "$1" in
    egress) run_egress "$@" ;;
    relay) run_relay "$@" ;;
    client) run_client "$@" ;;
    *) usage; exit 2 ;;
  esac
}

main "$@"
