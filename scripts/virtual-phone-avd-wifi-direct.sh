#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage:
  scripts/virtual-phone-avd-wifi-direct.sh <serial_a> <serial_b> <package> [evidence_dir]

Environment:
  SP3_ADB_BIN             adb executable, default: adb
  SP3_EMULATOR_BIN        emulator executable, default: emulator
  SP3_WIFI_DIRECT_PORT    TCP project port, default: 47117
  SP3_WIFI_DIRECT_PSK     64 hex chars, default: deterministic CI key
  SP3_WIFI_DIRECT_TIMEOUT poll timeout seconds, default: 90

This is Android Emulator/framework evidence, not physical-device evidence.
Requires Android Emulator 36.5+ where emulator P2P support is available.
EOF
}

[[ $# -ge 3 && $# -le 4 ]] || { usage; exit 2; }

SERIAL_A="$1"
SERIAL_B="$2"
PACKAGE="$3"
ADB="${SP3_ADB_BIN:-adb}"
EMULATOR="${SP3_EMULATOR_BIN:-emulator}"
PORT="${SP3_WIFI_DIRECT_PORT:-47117}"
PSK="${SP3_WIFI_DIRECT_PSK:-1111111111111111111111111111111111111111111111111111111111111111}"
TIMEOUT_SECONDS="${SP3_WIFI_DIRECT_TIMEOUT:-90}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
EVIDENCE="${4:-evidence/virtual-phone-avd-wifi-direct-${STAMP}}"
mkdir -p "$EVIDENCE"

command -v "$ADB" >/dev/null
command -v "$EMULATOR" >/dev/null
command -v sha256sum >/dev/null

[[ "$PSK" =~ ^[0-9a-fA-F]{64}$ ]] || {
  echo "SP3_WIFI_DIRECT_PSK must be exactly 64 hex chars" >&2
  exit 2
}
[[ "$PORT" =~ ^[0-9]+$ ]] || exit 2
(( PORT >= 1024 && PORT <= 65535 )) || exit 2

EMULATOR_VERSION="$("$EMULATOR" -version 2>&1 | sed -n 's/^Android emulator version \([0-9][0-9.]*\).*/\1/p' | head -n 1)"
test -n "$EMULATOR_VERSION"
python3 - "$EMULATOR_VERSION" <<'PY'
import sys
parts = [int(x) for x in sys.argv[1].split('.')[:3]]
parts += [0] * (3 - len(parts))
if tuple(parts) < (36, 5, 0):
    raise SystemExit(f"Android Emulator 36.5+ required, got {sys.argv[1]}")
PY

adb_run() {
  local serial="$1"
  shift
  "$ADB" -s "$serial" "$@"
}

api_level() {
  adb_run "$1" shell getprop ro.build.version.sdk | tr -d '\r'
}

require_avd() {
  local serial="$1"
  local state api
  state="$(adb_run "$serial" get-state | tr -d '\r')"
  [[ "$state" == "device" ]] || {
    echo "$serial is not online" >&2
    return 1
  }

  adb_run "$serial" shell pm path "$PACKAGE" >/dev/null

  local features feature_flag p2p_service location_mode
  features="$(adb_run "$serial" shell pm list features | tr -d '\r')"
  feature_flag=false
  if grep -q 'feature:android.hardware.wifi.direct' <<<"$features"; then
    feature_flag=true
  fi

  adb_run "$serial" shell svc wifi enable >/dev/null 2>&1 || true
  if ! adb_run "$serial" shell cmd location set-location-enabled true \
      >/dev/null 2>&1; then
    adb_run "$serial" shell settings put secure location_mode 3
  fi
  sleep 2

  location_mode="$(
    adb_run "$serial" shell settings get secure location_mode \
      | tr -d '\r'
  )"
  if [[ -z "$location_mode" || "$location_mode" == "0" ]]; then
    echo "$serial location mode is disabled" >&2
    return 1
  fi

  p2p_service="$(
    adb_run "$serial" shell service check wifip2p 2>&1 \
      | tr -d '\r' \
      || true
  )"
  if ! grep -qi 'found' <<<"$p2p_service"; then
    echo "$serial lacks a live wifip2p service: $p2p_service" >&2
    return 1
  fi

  echo "WIFI_DIRECT_AVD_CAPABILITY serial=$serial feature_flag=$feature_flag location_mode=$location_mode p2p_service=found"

  api="$(api_level "$serial")"
  if (( api >= 33 )); then
    adb_run "$serial" shell pm grant       "$PACKAGE" android.permission.NEARBY_WIFI_DEVICES
  else
    adb_run "$serial" shell pm grant       "$PACKAGE" android.permission.ACCESS_FINE_LOCATION
  fi
  if (( api >= 37 )); then
    adb_run "$serial" shell pm grant       "$PACKAGE" android.permission.ACCESS_LOCAL_NETWORK
  fi
}

snapshot() {
  local serial="$1"
  local prefix="$2"
  adb_run "$serial" shell getprop >"$EVIDENCE/${prefix}-getprop.txt"
  adb_run "$serial" shell pm list features >"$EVIDENCE/${prefix}-features.txt"
  adb_run "$serial" shell service check wifip2p >"$EVIDENCE/${prefix}-wifip2p-service.txt" 2>&1 || true
  adb_run "$serial" shell settings get secure location_mode >"$EVIDENCE/${prefix}-location-mode.txt" || true
  adb_run "$serial" shell ip addr >"$EVIDENCE/${prefix}-ip-addr.txt" || true
  adb_run "$serial" shell ip route >"$EVIDENCE/${prefix}-ip-route.txt" || true
  adb_run "$serial" shell dumpsys wifi >"$EVIDENCE/${prefix}-wifi.txt" || true
  adb_run "$serial" shell dumpsys wifip2p >"$EVIDENCE/${prefix}-wifip2p.txt" || true
  adb_run "$serial" shell dumpsys connectivity >"$EVIDENCE/${prefix}-connectivity.txt" || true
  adb_run "$serial" logcat -d -v threadtime -s SP3WifiDirect:I '*:S'     >"$EVIDENCE/${prefix}-wifi-direct-logcat.txt" || true
}

pass_line() {
  local serial="$1"
  adb_run "$serial" logcat -d -v brief -s SP3WifiDirect:I '*:S'     | tr -d '\r'     | grep 'WIFI_DIRECT_PAIR_PASS'     | tail -n 1
}

require_avd "$SERIAL_A"
require_avd "$SERIAL_B"

adb_run "$SERIAL_A" logcat -c
adb_run "$SERIAL_B" logcat -c

OWNER_COMPONENT="$PACKAGE/.WifiDirectPairCourtActivity"
adb_run "$SERIAL_A" shell am start -W   -n "$OWNER_COMPONENT"   --es dev.nolane.sanpham3.recoverylab.WIFI_DIRECT_ROLE owner   --es dev.nolane.sanpham3.recoverylab.WIFI_DIRECT_NODE_ID 200   --es dev.nolane.sanpham3.recoverylab.WIFI_DIRECT_PSK "$PSK"   --ei dev.nolane.sanpham3.recoverylab.WIFI_DIRECT_PORT "$PORT"   | tee "$EVIDENCE/owner-am-start.txt"

sleep 2

adb_run "$SERIAL_B" shell am start -W   -n "$OWNER_COMPONENT"   --es dev.nolane.sanpham3.recoverylab.WIFI_DIRECT_ROLE client   --es dev.nolane.sanpham3.recoverylab.WIFI_DIRECT_NODE_ID 100   --es dev.nolane.sanpham3.recoverylab.WIFI_DIRECT_PSK "$PSK"   --ei dev.nolane.sanpham3.recoverylab.WIFI_DIRECT_PORT "$PORT"   | tee "$EVIDENCE/client-am-start.txt"

OWNER_PASS=""
CLIENT_PASS=""
for _ in $(seq 1 "$TIMEOUT_SECONDS"); do
  OWNER_PASS="$(pass_line "$SERIAL_A" || true)"
  CLIENT_PASS="$(pass_line "$SERIAL_B" || true)"
  if [[ -n "$OWNER_PASS" && -n "$CLIENT_PASS" ]]; then
    break
  fi

  if adb_run "$SERIAL_A" logcat -d -v brief -s SP3WifiDirect:I '*:S'       | grep -q 'WIFI_DIRECT_PAIR_FAIL'; then
    break
  fi
  if adb_run "$SERIAL_B" logcat -d -v brief -s SP3WifiDirect:I '*:S'       | grep -q 'WIFI_DIRECT_PAIR_FAIL'; then
    break
  fi
  sleep 1
done

snapshot "$SERIAL_A" avd-a
snapshot "$SERIAL_B" avd-b

if [[ -z "$OWNER_PASS" || -z "$CLIENT_PASS" ]]; then
  echo "Wi-Fi Direct pair court did not PASS on both AVDs" >&2
  cat "$EVIDENCE/avd-a-wifi-direct-logcat.txt" >&2 || true
  cat "$EVIDENCE/avd-b-wifi-direct-logcat.txt" >&2 || true
  exit 1
fi

grep -q 'role=owner' <<<"$OWNER_PASS"
grep -q 'local_node=200' <<<"$OWNER_PASS"
grep -q 'peer_node=100' <<<"$OWNER_PASS"
grep -q 'group_owner=true' <<<"$OWNER_PASS"
grep -q 'evidence_level=ANDROID_AVD' <<<"$OWNER_PASS"

grep -q 'role=client' <<<"$CLIENT_PASS"
grep -q 'local_node=100' <<<"$CLIENT_PASS"
grep -q 'peer_node=200' <<<"$CLIENT_PASS"
grep -q 'group_owner=false' <<<"$CLIENT_PASS"
grep -q 'evidence_level=ANDROID_AVD' <<<"$CLIENT_PASS"

OWNER_CHALLENGE="$(sed -n 's/.*challenge=\([0-9a-f][0-9a-f]*\).*/\1/p' <<<"$OWNER_PASS")"
CLIENT_CHALLENGE="$(sed -n 's/.*challenge=\([0-9a-f][0-9a-f]*\).*/\1/p' <<<"$CLIENT_PASS")"
test -n "$OWNER_CHALLENGE"
test "$OWNER_CHALLENGE" = "$CLIENT_CHALLENGE"

{
  echo "timestamp_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "git_commit=$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "emulator_version=$EMULATOR_VERSION"
  echo "serial_a=$SERIAL_A"
  echo "serial_b=$SERIAL_B"
  echo "api_a=$(api_level "$SERIAL_A")"
  echo "api_b=$(api_level "$SERIAL_B")"
  echo "package=$PACKAGE"
  echo "port=$PORT"
  echo "owner_node=200"
  echo "client_node=100"
  echo "challenge=$OWNER_CHALLENGE"
  echo "evidence_level=ANDROID_AVD"
} >"$EVIDENCE/metadata.txt"

(
  cd "$EVIDENCE"
  find . -maxdepth 1 -type f ! -name SHA256SUMS -print0     | sort -z     | xargs -0 sha256sum >SHA256SUMS
)

echo "F5_AVD_WIFI_DIRECT_PAIR_PASS owner=$SERIAL_A client=$SERIAL_B challenge=$OWNER_CHALLENGE evidence=$EVIDENCE"
