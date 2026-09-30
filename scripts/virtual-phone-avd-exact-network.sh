#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage:
  scripts/virtual-phone-avd-exact-network.sh <serial_a> <serial_b> <package> [evidence_dir]

Runs the Recovery Lab ExactNetworkProbeActivity on two already-running Android
AVDs. A PASS requires the activity to report the same selected networkHandle
from the exact Network-bound DNS probe result.

This is ANDROID_AVD evidence only when executed against real Android emulators.
EOF
}

[[ $# -ge 3 && $# -le 4 ]] || { usage; exit 2; }

SERIAL_A="$1"
SERIAL_B="$2"
PACKAGE="$3"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
EVIDENCE="${4:-evidence/virtual-phone-avd-exact-network-$STAMP}"
COMPONENT="$PACKAGE/.ExactNetworkProbeActivity"
mkdir -p "$EVIDENCE"

command -v adb >/dev/null 2>&1 || {
  echo "adb is required" >&2
  exit 2
}
command -v sha256sum >/dev/null 2>&1 || {
  echo "sha256sum is required" >&2
  exit 2
}

run_one() {
  local serial="$1"
  local prefix="$2"
  local output handle result_handle sdk local_permission

  test "$(adb -s "$serial" get-state | tr -d '\r')" = device
  adb -s "$serial" shell pm path "$PACKAGE" >/dev/null

  sdk="$(adb -s "$serial" shell getprop ro.build.version.sdk | tr -d '\r')"
  if [[ "$sdk" =~ ^[0-9]+$ ]] && (( sdk >= 37 )); then
    adb -s "$serial" shell pm grant       "$PACKAGE" android.permission.ACCESS_LOCAL_NETWORK
  fi

  adb -s "$serial" logcat -c
  adb -s "$serial" shell am start -W -n "$COMPONENT"     >"$EVIDENCE/${prefix}-am-start.txt"

  output=""
  for _ in $(seq 1 30); do
    output="$(adb -s "$serial" logcat -d -s SP3ExactNetwork:I '*:S' | tr -d '\r')"
    if grep -q 'EXACT_NETWORK_PASS' <<<"$output"; then
      break
    fi
    if grep -q 'EXACT_NETWORK_FAIL' <<<"$output"; then
      break
    fi
    sleep 1
  done

  printf '%s\n' "$output" >"$EVIDENCE/${prefix}-exact-network-logcat.txt"
  adb -s "$serial" shell dumpsys connectivity     >"$EVIDENCE/${prefix}-connectivity.txt" || true
  adb -s "$serial" shell ip route     >"$EVIDENCE/${prefix}-route.txt" || true
  adb -s "$serial" shell dumpsys package "$PACKAGE"     >"$EVIDENCE/${prefix}-package.txt" || true

  if ! grep -q 'EXACT_NETWORK_PASS' <<<"$output"; then
    echo "exact-Network court failed serial=$serial" >&2
    return 1
  fi

  handle="$(sed -n 's/.*network_handle=\([0-9][0-9]*\).*/\1/p' <<<"$output" | tail -n 1)"
  result_handle="$(sed -n 's/.*result_handle=\([0-9][0-9]*\).*/\1/p' <<<"$output" | tail -n 1)"
  local_permission="$(sed -n 's/.*local_network_permission=\([^ ]*\).*/\1/p' <<<"$output" | tail -n 1)"

  test -n "$handle"
  test -n "$result_handle"
  test "$handle" = "$result_handle"
  if [[ "$sdk" =~ ^[0-9]+$ ]] && (( sdk >= 37 )); then
    test "$local_permission" = granted
  fi

  echo "PASS serial=$serial sdk=$sdk network_handle=$handle result_handle=$result_handle local_network_permission=${local_permission:-not_reported}"     | tee -a "$EVIDENCE/results.txt"
}

: >"$EVIDENCE/results.txt"
run_one "$SERIAL_A" avd-a
run_one "$SERIAL_B" avd-b

{
  echo "timestamp_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "git_commit=$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "serial_a=$SERIAL_A"
  echo "serial_b=$SERIAL_B"
  echo "package=$PACKAGE"
  echo "component=$COMPONENT"
  echo "evidence_level=ANDROID_AVD"
} >"$EVIDENCE/metadata.txt"

(
  cd "$EVIDENCE"
  find . -maxdepth 1 -type f ! -name SHA256SUMS -print0     | sort -z     | xargs -0 sha256sum >SHA256SUMS
)

echo "F5_EXACT_NETWORK_AVD_PASS evidence=$EVIDENCE devices=2"
