#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage:
  SP3_PEER_PSK_HEX=<64_hex_psk> scripts/g7-physical-role.sh egress <bind_addr> <node_id> <expected_client_node> [evidence_dir]
  scripts/g7-physical-role.sh shaper <listen_addr> <upstream_ip:port> <aggregate_bps> <chunk_bytes> <outage_period_ms> <outage_down_ms> [evidence_dir]
  SP3_PEER_PSK_HEX=<64_hex_psk> scripts/g7-physical-role.sh client <shaper_ip:port> <node_id> <expected_egress_node> <hostname> [evidence_dir]
EOF
}

prepare() {
  local role="$1"
  local requested="${2:-}"
  local stamp
  stamp="$(date -u +%Y%m%dT%H%M%SZ)"
  if [[ -n "$requested" ]]; then
    EVIDENCE_DIR="$requested"
  else
    EVIDENCE_DIR="evidence/g7-${role}-${stamp}"
  fi
  mkdir -p "$EVIDENCE_DIR"
  LOG_FILE="$EVIDENCE_DIR/${role}.log"
  META_FILE="$EVIDENCE_DIR/metadata.txt"
}

require_psk() {
  : "${SP3_PEER_PSK_HEX:?SP3_PEER_PSK_HEX must contain the shared 64-hex PSK}"
  if [[ ! "$SP3_PEER_PSK_HEX" =~ ^[0-9a-fA-F]{64}$ ]]; then
    echo "SP3_PEER_PSK_HEX must be exactly 64 hexadecimal characters" >&2
    exit 2
  fi
}

build_bins() {
  cargo build --release -p g7-shaper-proxy -p peer-egress-cli
  SHAPER_BIN="./target/release/g7-shaper-proxy"
  EGRESS_BIN="./target/release/peer-egress-cli"
  test -x "$SHAPER_BIN"
  test -x "$EGRESS_BIN"
}

write_meta() {
  local role="$1"
  shift
  {
    echo "timestamp_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "git_commit=$(git rev-parse HEAD)"
    echo "role=$role"
    echo "hostname=$(hostname 2>/dev/null || true)"
    echo "os=$(uname -a 2>/dev/null || true)"
    echo "shaper_sha256=$(sha256sum "$SHAPER_BIN" | awk '{print $1}')"
    echo "peer_egress_sha256=$(sha256sum "$EGRESS_BIN" | awk '{print $1}')"
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

finalize() {
  (
    cd "$EVIDENCE_DIR"
    find . -maxdepth 1 -type f ! -name SHA256SUMS -print0       | sort -z       | xargs -0 sha256sum >SHA256SUMS
  )
}

cmd_egress() {
  [[ $# -eq 4 || $# -eq 5 ]] || { usage; exit 2; }
  require_psk
  local bind="$2" node="$3" expected_client="$4" evidence="${5:-}"
  prepare egress "$evidence"
  write_meta egress "bind_addr=$bind" "node_id=$node" "expected_client_node=$expected_client"
  run_logged "$EGRESS_BIN" server "$bind" "$node" -
  grep -q "authenticated peer node_id=$expected_client" "$LOG_FILE"
  grep -q "request served" "$LOG_FILE"
  echo "G7_ROLE_PASS role=egress node_id=$node client_node=$expected_client"
  finalize
}

cmd_shaper() {
  [[ $# -eq 7 || $# -eq 8 ]] || { usage; exit 2; }
  local listen="$2" upstream="$3" bps="$4" chunk="$5" period="$6" down="$7" evidence="${8:-}"
  prepare shaper "$evidence"
  write_meta shaper "listen_addr=$listen" "upstream_addr=$upstream" "aggregate_bps=$bps" "chunk_bytes=$chunk" "outage_period_ms=$period" "outage_down_ms=$down"
  run_logged "$SHAPER_BIN" "$listen" "$upstream" "$bps" "$chunk" "$period" "$down"
  grep -q "G7_SHAPER_PASS aggregate_target_bps=$bps" "$LOG_FILE"

  local observed total
  observed="$(sed -n 's/.*wall_observed_bps=\([0-9][0-9]*\).*/\1/p' "$LOG_FILE" | tail -n 1)"
  total="$(sed -n 's/.*total_bytes=\([0-9][0-9]*\).*/\1/p' "$LOG_FILE" | tail -n 1)"

  test -n "$observed"
  test -n "$total"
  test "$total" -gt 0

  if test "$observed" -gt "$bps"; then
    echo "observed aggregate rate $observed exceeded configured cap $bps" >&2
    exit 3
  fi

  echo "G7_ROLE_PASS role=shaper target_bps=$bps observed_bps=$observed total_bytes=$total"
  finalize
}

cmd_client() {
  [[ $# -eq 5 || $# -eq 6 ]] || { usage; exit 2; }
  require_psk
  local peer="$2" node="$3" expected_egress="$4" host="$5" evidence="${6:-}"
  prepare client "$evidence"
  write_meta client "shaper_addr=$peer" "node_id=$node" "expected_egress_node=$expected_egress" "hostname=$host"
  run_logged "$EGRESS_BIN" client "$peer" "$node" - "$host"
  grep -q "authenticated egress peer node_id=$expected_egress" "$LOG_FILE"
  grep -q "resolved $host through peer:" "$LOG_FILE"
  echo "G7_ROLE_PASS role=client node_id=$node egress_node=$expected_egress hostname=$host"
  finalize
}

main() {
  [[ $# -ge 1 ]] || { usage; exit 2; }
  build_bins
  case "$1" in
    egress) cmd_egress "$@" ;;
    shaper) cmd_shaper "$@" ;;
    client) cmd_client "$@" ;;
    *) usage; exit 2 ;;
  esac
}

main "$@"
