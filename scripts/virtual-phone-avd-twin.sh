#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage:
  scripts/virtual-phone-avd-twin.sh <avd_a> <avd_b> [apk]

Environment:
  SP3_AVD_MEMORY_MB   RAM per AVD, default 2048
  SP3_AVD_NETSPEED    emulator -netspeed value, default edge
  SP3_AVD_NETDELAY    emulator -netdelay value, default 200
  SP3_KEEP_AVD        set to 1 to leave AVDs running after snapshots

This harness is Android-framework evidence, not physical-device evidence.
EOF
}

[[ $# -ge 2 && $# -le 3 ]] || { usage; exit 2; }

AVD_A="$1"
AVD_B="$2"
APK="${3:-}"
MEMORY="${SP3_AVD_MEMORY_MB:-2048}"
NETSPEED="${SP3_AVD_NETSPEED:-edge}"
NETDELAY="${SP3_AVD_NETDELAY:-200}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
EVIDENCE="evidence/virtual-phone-avd-${STAMP}"
mkdir -p "$EVIDENCE"

command -v emulator >/dev/null
command -v adb >/dev/null

EMULATOR_BIN="$(command -v emulator)"

cleanup() {
  if [[ "${SP3_KEEP_AVD:-0}" != "1" ]]; then
    adb -s emulator-5554 emu kill >/dev/null 2>&1 || true
    adb -s emulator-5556 emu kill >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

wait_boot() {
  local serial="$1"
  adb -s "$serial" wait-for-device
  for _ in $(seq 1 180); do
    local boot
    boot="$(adb -s "$serial" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')"
    if [[ "$boot" == "1" ]]; then
      return 0
    fi
    sleep 1
  done
  echo "Timed out waiting for $serial" >&2
  return 1
}

"$EMULATOR_BIN" "@$AVD_A"   -port 5554   -no-snapshot   -no-boot-anim   -memory "$MEMORY"   -netspeed "$NETSPEED"   -netdelay "$NETDELAY"   -tcpdump "$EVIDENCE/avd-a.pcap"   >"$EVIDENCE/avd-a-emulator.log" 2>&1 &
PID_A=$!

"$EMULATOR_BIN" "@$AVD_B"   -port 5556   -no-snapshot   -no-boot-anim   -memory "$MEMORY"   -netspeed "$NETSPEED"   -netdelay "$NETDELAY"   -tcpdump "$EVIDENCE/avd-b.pcap"   >"$EVIDENCE/avd-b-emulator.log" 2>&1 &
PID_B=$!

echo "$PID_A" >"$EVIDENCE/avd-a.pid"
echo "$PID_B" >"$EVIDENCE/avd-b.pid"

wait_boot emulator-5554
wait_boot emulator-5556

if [[ -n "$APK" ]]; then
  test -f "$APK"
  adb -s emulator-5554 install -r "$APK" | tee "$EVIDENCE/avd-a-install.txt"
  adb -s emulator-5556 install -r "$APK" | tee "$EVIDENCE/avd-b-install.txt"
fi

snapshot() {
  local serial="$1"
  local prefix="$2"

  adb -s "$serial" shell getprop >"$EVIDENCE/${prefix}-getprop.txt"
  adb -s "$serial" shell ip addr >"$EVIDENCE/${prefix}-ip-addr.txt" || true
  adb -s "$serial" shell ip route >"$EVIDENCE/${prefix}-ip-route.txt" || true
  adb -s "$serial" shell dumpsys connectivity >"$EVIDENCE/${prefix}-connectivity.txt" || true
  adb -s "$serial" shell pm list features >"$EVIDENCE/${prefix}-features.txt" || true
  adb -s "$serial" shell dumpsys wifi >"$EVIDENCE/${prefix}-wifi.txt" || true
  adb -s "$serial" shell dumpsys bluetooth_manager >"$EVIDENCE/${prefix}-bluetooth.txt" || true
}

snapshot emulator-5554 avd-a
snapshot emulator-5556 avd-b

{
  echo "timestamp_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "git_commit=$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "avd_a=$AVD_A"
  echo "avd_b=$AVD_B"
  echo "memory_mb=$MEMORY"
  echo "netspeed=$NETSPEED"
  echo "netdelay=$NETDELAY"
  echo "apk=${APK:-none}"
  echo "evidence_level=ANDROID_AVD"
} >"$EVIDENCE/metadata.txt"

(
  cd "$EVIDENCE"
  find . -maxdepth 1 -type f ! -name SHA256SUMS -print0     | sort -z     | xargs -0 sha256sum >SHA256SUMS
)

echo "VIRTUAL_PHONE_AVD_READY evidence=$EVIDENCE"
echo "A=emulator-5554 B=emulator-5556"

if [[ "${SP3_KEEP_AVD:-0}" == "1" ]]; then
  trap - EXIT
  echo "SP3_KEEP_AVD=1: emulators left running"
fi
