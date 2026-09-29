#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage:
  scripts/virtual-phone-avd-scenarios.sh <serial_a> <serial_b> <package> [evidence_dir]

Environment:
  SP3_ADB_BIN                 adb executable, default: adb
  SP3_SCENARIO_PERMISSIONS    space-separated runtime permissions
  SP3_LAUNCH_COMPONENT        optional package/activity component
  SP3_SCENARIO_WAIT_MS        delay after app launch/state change, default: 500
  SP3_REQUIRE_PERMISSION_EXERCISE
                              1 (default) fails if no permission can be exercised

The driver records ANDROID_AVD evidence. A mock-adb CI court validates the
driver state machine only and is not Android framework evidence.
EOF
}

[[ $# -ge 3 && $# -le 4 ]] || { usage; exit 2; }

SERIAL_A="$1"
SERIAL_B="$2"
PACKAGE="$3"
ADB="${SP3_ADB_BIN:-adb}"
WAIT_MS="${SP3_SCENARIO_WAIT_MS:-500}"
REQUIRE_EXERCISE="${SP3_REQUIRE_PERMISSION_EXERCISE:-1}"
PERMISSIONS="${SP3_SCENARIO_PERMISSIONS:-android.permission.ACCESS_FINE_LOCATION android.permission.BLUETOOTH_SCAN android.permission.BLUETOOTH_CONNECT android.permission.NEARBY_WIFI_DEVICES}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
EVIDENCE="${4:-evidence/virtual-phone-avd-scenarios-${STAMP}}"
mkdir -p "$EVIDENCE"

command -v "$ADB" >/dev/null 2>&1 || {
  echo "adb executable not found: $ADB" >&2
  exit 2
}
command -v sha256sum >/dev/null 2>&1 || {
  echo "sha256sum is required" >&2
  exit 2
}

EXERCISED=0
SKIPPED=0
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

permission_state() {
  local serial="$1"
  local permission="$2"
  local dump line
  dump="$(adb_run "$serial" shell dumpsys package "$PACKAGE" 2>/dev/null | tr -d '\r')"
  line="$(printf '%s\n' "$dump" | grep -F "${permission}:" | head -n 1 || true)"

  if [[ -z "$line" ]]; then
    printf 'unknown'
  elif [[ "$line" == *"granted=true"* ]]; then
    printf 'granted'
  elif [[ "$line" == *"granted=false"* ]]; then
    printf 'denied'
  else
    printf 'unknown'
  fi
}

set_permission() {
  local serial="$1"
  local permission="$2"
  local state="$3"

  case "$state" in
    granted)
      adb_run "$serial" shell pm grant "$PACKAGE" "$permission" >/dev/null
      ;;
    denied)
      adb_run "$serial" shell pm revoke "$PACKAGE" "$permission" >/dev/null
      ;;
    *)
      echo "invalid target permission state: $state" >&2
      return 2
      ;;
  esac
}

verify_permission() {
  local serial="$1"
  local permission="$2"
  local expected="$3"
  local actual
  actual="$(permission_state "$serial" "$permission")"
  if [[ "$actual" != "$expected" ]]; then
    echo "permission verification failed serial=$serial permission=$permission expected=$expected actual=$actual" >&2
    return 1
  fi
}

launch_app() {
  local serial="$1"

  adb_run "$serial" shell am force-stop "$PACKAGE" >/dev/null 2>&1 || true
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
  adb_run "$serial" shell dumpsys connectivity >"$EVIDENCE/${prefix}-connectivity.txt" || true
  adb_run "$serial" shell ip route >"$EVIDENCE/${prefix}-route.txt" || true
  adb_run "$serial" logcat -d -t 300 >"$EVIDENCE/${prefix}-logcat.txt" || true
}

run_permission_scenario() {
  local serial="$1"
  local permission="$2"
  local baseline target restored label
  baseline="$(permission_state "$serial" "$permission")"
  label="$(safe_name "$permission")"

  if [[ "$baseline" == "unknown" ]]; then
    echo "SKIP serial=$serial permission=$permission reason=not_requested_or_unobservable"       | tee -a "$EVIDENCE/scenario-results.txt"
    SKIPPED=$((SKIPPED + 1))
    return 0
  fi

  if [[ "$baseline" == "granted" ]]; then
    target="denied"
  else
    target="granted"
  fi

  echo "BEGIN serial=$serial permission=$permission baseline=$baseline target=$target"     | tee -a "$EVIDENCE/scenario-results.txt"

  if ! set_permission "$serial" "$permission" "$target"; then
    echo "FAIL serial=$serial permission=$permission phase=transition"       | tee -a "$EVIDENCE/scenario-results.txt"
    FAILED=$((FAILED + 1))
    return 1
  fi
  verify_permission "$serial" "$permission" "$target"
  launch_app "$serial"
  snapshot "$serial" "${label}-${target}"

  set_permission "$serial" "$permission" "$baseline"
  verify_permission "$serial" "$permission" "$baseline"
  restored="$(permission_state "$serial" "$permission")"
  snapshot "$serial" "${label}-restored"

  echo "PASS serial=$serial permission=$permission transitioned=$target restored=$restored"     | tee -a "$EVIDENCE/scenario-results.txt"
  EXERCISED=$((EXERCISED + 1))
}

run_device() {
  local serial="$1"
  require_device "$serial"
  snapshot "$serial" baseline

  local permission
  for permission in $PERMISSIONS; do
    run_permission_scenario "$serial" "$permission"
  done
}

: >"$EVIDENCE/scenario-results.txt"
run_device "$SERIAL_A"
run_device "$SERIAL_B"

if [[ "$FAILED" -ne 0 ]]; then
  echo "scenario failures=$FAILED" >&2
  exit 1
fi

if [[ "$REQUIRE_EXERCISE" == "1" && "$EXERCISED" -eq 0 ]]; then
  echo "no observable runtime permission could be exercised" >&2
  exit 1
fi

{
  echo "timestamp_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "git_commit=$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "serial_a=$SERIAL_A"
  echo "serial_b=$SERIAL_B"
  echo "package=$PACKAGE"
  echo "permissions=$PERMISSIONS"
  echo "exercised=$EXERCISED"
  echo "skipped=$SKIPPED"
  echo "failed=$FAILED"
  echo "evidence_level=ANDROID_AVD"
} >"$EVIDENCE/metadata.txt"

(
  cd "$EVIDENCE"
  find . -maxdepth 1 -type f ! -name SHA256SUMS -print0     | sort -z     | xargs -0 sha256sum >SHA256SUMS
)

echo "F5_AVD_SCENARIO_PASS evidence=$EVIDENCE exercised=$EXERCISED skipped=$SKIPPED restored=true"
