#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage:
  scripts/virtual-phone-avd-failure-matrix.sh <serial_a> <serial_b> <package> [evidence_dir]

Environment:
  SP3_ADB_BIN           adb executable, default: adb
  SP3_LAUNCH_COMPONENT optional package/activity component
  SP3_MATRIX_WAIT_MS    wait after state transition, default: 500
  SP3_REQUIRE_DOZE      1 fails if device-idle commands are unavailable;
                        0 (default) records an explicit SKIP
  SP3_EVIDENCE_LEVEL    evidence label, default: ANDROID_AVD

The script is intended for Android AVDs. Fake-adb courts must override
SP3_EVIDENCE_LEVEL so modeled state-machine evidence cannot be mistaken for
Android framework evidence.
EOF
}

[[ $# -ge 3 && $# -le 4 ]] || { usage; exit 2; }

SERIAL_A="$1"
SERIAL_B="$2"
PACKAGE="$3"
ADB="${SP3_ADB_BIN:-adb}"
WAIT_MS="${SP3_MATRIX_WAIT_MS:-500}"
REQUIRE_DOZE="${SP3_REQUIRE_DOZE:-0}"
EVIDENCE_LEVEL="${SP3_EVIDENCE_LEVEL:-ANDROID_AVD}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
EVIDENCE="${4:-evidence/virtual-phone-avd-failure-${STAMP}}"
mkdir -p "$EVIDENCE"

command -v "$ADB" >/dev/null 2>&1 || {
  echo "adb executable not found: $ADB" >&2
  exit 2
}
command -v sha256sum >/dev/null 2>&1 || {
  echo "sha256sum is required" >&2
  exit 2
}

RESTART_PASS=0
DOZE_PASS=0
DOZE_SKIP=0
FAILED=0

sleep_ms() {
  local ms="$1"
  if [[ "$ms" == "0" ]]; then
    return 0
  fi
  python3 - "$ms" <<'PY'
import sys, time
time.sleep(int(sys.argv[1]) / 1000.0)
PY
}

safe_name() {
  printf '%s' "$1" | tr -c '[:alnum:]_-' '_'
}

adb_run() {
  local serial="$1"
  shift
  "$ADB" -s "$serial" "$@"
}

require_device() {
  local serial="$1"
  local state
  state="$(adb_run "$serial" get-state 2>/dev/null | tr -d '\r')"
  [[ "$state" == "device" ]] || {
    echo "$serial is not an online adb device (state=$state)" >&2
    return 1
  }
  adb_run "$serial" shell pm path "$PACKAGE" >/dev/null
}

launch_app() {
  local serial="$1"
  if [[ -n "${SP3_LAUNCH_COMPONENT:-}" ]]; then
    adb_run "$serial" shell am start -W -n "$SP3_LAUNCH_COMPONENT" >/dev/null
  else
    adb_run "$serial" shell monkey -p "$PACKAGE" -c android.intent.category.LAUNCHER 1 >/dev/null
  fi
  sleep_ms "$WAIT_MS"
}

snapshot() {
  local serial="$1"
  local label="$2"
  local prefix
  prefix="$(safe_name "${serial}-${label}")"

  adb_run "$serial" shell dumpsys package "$PACKAGE" >"$EVIDENCE/${prefix}-package.txt" || true
  adb_run "$serial" shell dumpsys activity processes >"$EVIDENCE/${prefix}-activity-processes.txt" || true
  adb_run "$serial" shell dumpsys connectivity >"$EVIDENCE/${prefix}-connectivity.txt" || true
  adb_run "$serial" shell dumpsys deviceidle >"$EVIDENCE/${prefix}-deviceidle.txt" || true
  adb_run "$serial" logcat -d -t 300 >"$EVIDENCE/${prefix}-logcat.txt" || true
}

deep_idle_state() {
  local serial="$1"
  adb_run "$serial" shell dumpsys deviceidle get deep 2>/dev/null     | tr -d '\r'     | tail -n 1     | tr '[:lower:]' '[:upper:]'
}

set_deep_idle() {
  local serial="$1"
  adb_run "$serial" shell dumpsys deviceidle force-idle deep >/dev/null
  sleep_ms "$WAIT_MS"
}

clear_forced_idle() {
  local serial="$1"
  adb_run "$serial" shell dumpsys deviceidle unforce >/dev/null
  sleep_ms "$WAIT_MS"
}

restore_idle_state() {
  local serial="$1"
  local baseline="$2"

  case "$baseline" in
    IDLE)
      set_deep_idle "$serial"
      ;;
    ACTIVE)
      clear_forced_idle "$serial"
      ;;
    *)
      clear_forced_idle "$serial" || true
      ;;
  esac
}

run_restart_scenario() {
  local serial="$1"

  launch_app "$serial"
  snapshot "$serial" restart-before

  adb_run "$serial" shell am force-stop "$PACKAGE" >/dev/null
  sleep_ms "$WAIT_MS"
  snapshot "$serial" restart-stopped

  launch_app "$serial"
  snapshot "$serial" restart-after

  echo "PASS serial=$serial scenario=app_restart restored=running"     | tee -a "$EVIDENCE/matrix-results.txt"
  RESTART_PASS=$((RESTART_PASS + 1))
}

run_doze_scenario() {
  local serial="$1"
  local baseline forced restored

  baseline="$(deep_idle_state "$serial" || true)"
  if [[ "$baseline" != "ACTIVE" && "$baseline" != "IDLE" ]]; then
    echo "SKIP serial=$serial scenario=deep_doze reason=unsupported baseline=$baseline"       | tee -a "$EVIDENCE/matrix-results.txt"
    DOZE_SKIP=$((DOZE_SKIP + 1))
    if [[ "$REQUIRE_DOZE" == "1" ]]; then
      FAILED=$((FAILED + 1))
      return 1
    fi
    return 0
  fi

  snapshot "$serial" doze-before

  if ! set_deep_idle "$serial"; then
    restore_idle_state "$serial" "$baseline"
    echo "FAIL serial=$serial scenario=deep_doze phase=force_idle"       | tee -a "$EVIDENCE/matrix-results.txt"
    FAILED=$((FAILED + 1))
    return 1
  fi

  forced="$(deep_idle_state "$serial" || true)"
  snapshot "$serial" doze-forced
  if [[ "$forced" != "IDLE" ]]; then
    restore_idle_state "$serial" "$baseline"
    echo "FAIL serial=$serial scenario=deep_doze phase=verify_idle actual=$forced"       | tee -a "$EVIDENCE/matrix-results.txt"
    FAILED=$((FAILED + 1))
    return 1
  fi

  restore_idle_state "$serial" "$baseline"
  restored="$(deep_idle_state "$serial" || true)"
  snapshot "$serial" doze-restored

  if [[ "$restored" != "$baseline" ]]; then
    echo "FAIL serial=$serial scenario=deep_doze phase=restore expected=$baseline actual=$restored"       | tee -a "$EVIDENCE/matrix-results.txt"
    FAILED=$((FAILED + 1))
    return 1
  fi

  echo "PASS serial=$serial scenario=deep_doze forced=$forced restored=$restored"     | tee -a "$EVIDENCE/matrix-results.txt"
  DOZE_PASS=$((DOZE_PASS + 1))
}

run_device() {
  local serial="$1"
  require_device "$serial"
  snapshot "$serial" baseline
  run_restart_scenario "$serial"
  run_doze_scenario "$serial"
}

: >"$EVIDENCE/matrix-results.txt"
run_device "$SERIAL_A"
run_device "$SERIAL_B"

if [[ "$FAILED" -ne 0 ]]; then
  echo "matrix failures=$FAILED" >&2
  exit 1
fi

{
  echo "timestamp_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "git_commit=$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "serial_a=$SERIAL_A"
  echo "serial_b=$SERIAL_B"
  echo "package=$PACKAGE"
  echo "restart_pass=$RESTART_PASS"
  echo "doze_pass=$DOZE_PASS"
  echo "doze_skip=$DOZE_SKIP"
  echo "failed=$FAILED"
  echo "evidence_level=$EVIDENCE_LEVEL"
} >"$EVIDENCE/metadata.txt"

(
  cd "$EVIDENCE"
  find . -maxdepth 1 -type f ! -name SHA256SUMS -print0     | sort -z     | xargs -0 sha256sum >SHA256SUMS
)

echo "F5_AVD_FAILURE_MATRIX_PASS evidence=$EVIDENCE restarts=$RESTART_PASS doze=$DOZE_PASS skipped=$DOZE_SKIP restored=true"
