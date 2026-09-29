#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage:
  SP3_PEER_PSK_HEX=<64_hex_psk> scripts/g6-physical-step.sh enqueue <spool> <bundle_id> <request_id> <hostname> <bulk|normal|urgent> <ttl_secs> [evidence_dir]
  SP3_PEER_PSK_HEX=<64_hex_psk> scripts/g6-physical-step.sh receive <bind_addr> <node_id> <expected_peer_node> <spool> <expected_bundle_id> [evidence_dir]
  SP3_PEER_PSK_HEX=<64_hex_psk> scripts/g6-physical-step.sh send <peer_addr> <node_id> <expected_peer_node> <spool> <expected_bundle_id> [evidence_dir]
  SP3_PEER_PSK_HEX=<64_hex_psk> scripts/g6-physical-step.sh egress <bind_addr> <node_id> <expected_peer_node> [evidence_dir]
  SP3_PEER_PSK_HEX=<64_hex_psk> scripts/g6-physical-step.sh dispatch <egress_addr> <node_id> <expected_peer_node> <request_spool> <request_bundle_id> <return_spool> <return_bundle_id> <return_ttl_secs> [evidence_dir]
  SP3_PEER_PSK_HEX=<64_hex_psk> scripts/g6-physical-step.sh inspect <spool> <expected_bundle_id|empty> [evidence_dir]
  SP3_PEER_PSK_HEX=<64_hex_psk> scripts/g6-physical-step.sh show-result <spool> <expected_bundle_id> <expected_request_id> [evidence_dir]
EOF
}

require_psk() {
  : "${SP3_PEER_PSK_HEX:?SP3_PEER_PSK_HEX must contain the shared 64-hex PSK}"
  if [[ ! "$SP3_PEER_PSK_HEX" =~ ^[0-9a-fA-F]{64}$ ]]; then
    echo "SP3_PEER_PSK_HEX must be exactly 64 hexadecimal characters" >&2
    exit 2
  fi
}

prepare() {
  local step="$1"
  local requested="${2:-}"
  local stamp
  stamp="$(date -u +%Y%m%dT%H%M%SZ)"
  if [[ -n "$requested" ]]; then
    EVIDENCE_DIR="$requested"
  else
    EVIDENCE_DIR="evidence/g6-${step}-${stamp}"
  fi
  mkdir -p "$EVIDENCE_DIR"
  LOG_FILE="$EVIDENCE_DIR/${step}.log"
  META_FILE="$EVIDENCE_DIR/metadata.txt"
}

build_bins() {
  cargo build --release -p recovery-lab-cli -p peer-egress-cli
  LAB_BIN="./target/release/recovery-lab-cli"
  EGRESS_BIN="./target/release/peer-egress-cli"
  test -x "$LAB_BIN"
  test -x "$EGRESS_BIN"
}

write_meta() {
  local step="$1"
  shift
  {
    echo "timestamp_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "git_commit=$(git rev-parse HEAD)"
    echo "step=$step"
    echo "hostname=$(hostname 2>/dev/null || true)"
    echo "os=$(uname -a 2>/dev/null || true)"
    echo "recovery_lab_sha256=$(sha256sum "$LAB_BIN" | awk '{print $1}')"
    echo "peer_egress_sha256=$(sha256sum "$EGRESS_BIN" | awk '{print $1}')"
    for item in "$@"; do
      echo "$item"
    done
  } >"$META_FILE"
}

spool_snapshot() {
  local label="$1"
  local spool="$2"
  local out="$EVIDENCE_DIR/${label}.txt"
  {
    echo "path=$spool"
    if [[ -f "$spool" ]]; then
      echo "exists=yes"
      echo "size_bytes=$(wc -c <"$spool" | tr -d ' ')"
      echo "sha256=$(sha256sum "$spool" | awk '{print $1}')"
    else
      echo "exists=no"
    fi
    "$LAB_BIN" inspect-spool "$spool"
  } >"$out"
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

cmd_enqueue() {
  [[ $# -eq 7 || $# -eq 8 ]] || { usage; exit 2; }
  local spool="$2" bundle="$3" request="$4" host="$5" priority="$6" ttl="$7" evidence="${8:-}"
  prepare enqueue "$evidence"
  write_meta enqueue "spool=$spool" "bundle_id=$bundle" "request_id=$request" "hostname=$host" "priority=$priority" "ttl_secs=$ttl"
  spool_snapshot before "$spool"
  run_logged "$LAB_BIN" enqueue "$spool" "$bundle" "$request" "$host" "$priority" "$ttl"
  spool_snapshot after "$spool"
  grep -q "bundle_id=$bundle" "$EVIDENCE_DIR/after.txt"
  echo "G6_STEP_PASS step=enqueue bundle_id=$bundle"
  finalize
}

cmd_receive() {
  [[ $# -eq 6 || $# -eq 7 ]] || { usage; exit 2; }
  local bind="$2" node="$3" expected_peer="$4" spool="$5" bundle="$6" evidence="${7:-}"
  prepare receive "$evidence"
  write_meta receive "bind_addr=$bind" "node_id=$node" "expected_peer_node=$expected_peer" "spool=$spool" "expected_bundle_id=$bundle"
  spool_snapshot before "$spool"
  run_logged "$LAB_BIN" custody-receive "$bind" "$node" - "$spool"
  spool_snapshot after "$spool"
  grep -q "peer_id=$expected_peer" "$LOG_FILE"
  grep -q "bundle_id=$bundle" "$LOG_FILE"
  grep -q "bundle_id=$bundle" "$EVIDENCE_DIR/after.txt"
  echo "G6_STEP_PASS step=receive node_id=$node peer_id=$expected_peer bundle_id=$bundle"
  finalize
}

cmd_send() {
  [[ $# -eq 6 || $# -eq 7 ]] || { usage; exit 2; }
  local peer="$2" node="$3" expected_peer="$4" spool="$5" bundle="$6" evidence="${7:-}"
  prepare send "$evidence"
  write_meta send "peer_addr=$peer" "node_id=$node" "expected_peer_node=$expected_peer" "spool=$spool" "expected_bundle_id=$bundle"
  spool_snapshot before "$spool"
  grep -q "bundle_id=$bundle" "$EVIDENCE_DIR/before.txt"
  run_logged "$LAB_BIN" custody-send "$peer" "$node" - "$spool"
  spool_snapshot after "$spool"
  grep -q "authenticated peer_id=$expected_peer" "$LOG_FILE"
  if grep -q "bundle_id=$bundle" "$EVIDENCE_DIR/after.txt"; then
    echo "bundle $bundle still present after custody ACK" >&2
    exit 3
  fi
  echo "G6_STEP_PASS step=send node_id=$node peer_id=$expected_peer bundle_id=$bundle"
  finalize
}

cmd_egress() {
  [[ $# -eq 4 || $# -eq 5 ]] || { usage; exit 2; }
  local bind="$2" node="$3" expected_peer="$4" evidence="${5:-}"
  prepare egress "$evidence"
  write_meta egress "bind_addr=$bind" "node_id=$node" "expected_peer_node=$expected_peer"
  run_logged "$EGRESS_BIN" server "$bind" "$node" -
  grep -q "authenticated peer node_id=$expected_peer" "$LOG_FILE"
  grep -q "request served" "$LOG_FILE"
  echo "G6_STEP_PASS step=egress node_id=$node peer_id=$expected_peer"
  finalize
}

cmd_dispatch() {
  [[ $# -eq 9 || $# -eq 10 ]] || { usage; exit 2; }
  local peer="$2" node="$3" expected_peer="$4" req_spool="$5" req_bundle="$6" ret_spool="$7" ret_bundle="$8" ret_ttl="$9" evidence="${10:-}"
  prepare dispatch "$evidence"
  write_meta dispatch "egress_addr=$peer" "node_id=$node" "expected_peer_node=$expected_peer" "request_spool=$req_spool" "request_bundle_id=$req_bundle" "return_spool=$ret_spool" "return_bundle_id=$ret_bundle" "return_ttl_secs=$ret_ttl"
  spool_snapshot request_before "$req_spool"
  spool_snapshot return_before "$ret_spool"
  grep -q "bundle_id=$req_bundle" "$EVIDENCE_DIR/request_before.txt"
  run_logged "$LAB_BIN" dispatch "$peer" "$node" - "$req_spool" "$ret_spool" "$ret_bundle" "$ret_ttl"
  spool_snapshot request_after "$req_spool"
  spool_snapshot return_after "$ret_spool"
  grep -q "egress peer_id=$expected_peer" "$LOG_FILE"
  grep -q "bundle_id=$ret_bundle" "$EVIDENCE_DIR/return_after.txt"
  if grep -q "bundle_id=$req_bundle" "$EVIDENCE_DIR/request_after.txt"; then
    echo "request bundle $req_bundle still present after terminal dispatch" >&2
    exit 3
  fi
  echo "G6_STEP_PASS step=dispatch node_id=$node egress_peer=$expected_peer return_bundle_id=$ret_bundle"
  finalize
}

cmd_inspect() {
  [[ $# -eq 3 || $# -eq 4 ]] || { usage; exit 2; }
  local spool="$2" expected="$3" evidence="${4:-}"
  prepare inspect "$evidence"
  write_meta inspect "spool=$spool" "expected=$expected"
  spool_snapshot current "$spool"
  cp "$EVIDENCE_DIR/current.txt" "$LOG_FILE"
  if [[ "$expected" == "empty" ]]; then
    grep -q "bundles=0" "$LOG_FILE"
  else
    grep -q "bundle_id=$expected" "$LOG_FILE"
  fi
  echo "G6_STEP_PASS step=inspect expected=$expected"
  finalize
}

cmd_show_result() {
  [[ $# -eq 4 || $# -eq 5 ]] || { usage; exit 2; }
  local spool="$2" bundle="$3" request="$4" evidence="${5:-}"
  prepare show-result "$evidence"
  write_meta show-result "spool=$spool" "expected_bundle_id=$bundle" "expected_request_id=$request"
  spool_snapshot before "$spool"
  run_logged "$LAB_BIN" show-result "$spool"
  grep -q "bundle_id=$bundle request_id=$request status=Ok" "$LOG_FILE"
  echo "G6_STEP_PASS step=show-result bundle_id=$bundle request_id=$request"
  finalize
}

main() {
  [[ $# -ge 1 ]] || { usage; exit 2; }
  require_psk
  build_bins
  case "$1" in
    enqueue) cmd_enqueue "$@" ;;
    receive) cmd_receive "$@" ;;
    send) cmd_send "$@" ;;
    egress) cmd_egress "$@" ;;
    dispatch) cmd_dispatch "$@" ;;
    inspect) cmd_inspect "$@" ;;
    show-result) cmd_show_result "$@" ;;
    *) usage; exit 2 ;;
  esac
}

main "$@"
