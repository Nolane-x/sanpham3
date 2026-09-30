#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage:
  scripts/virtual-phone-avd-camera-optical.sh <serial> <package> <expected_hex> [evidence_dir]

Environment:
  SP3_ADB_BIN                 adb executable, default: adb
  SP3_CAMERA_FRAME_COUNT      captured frames, default: 12
  SP3_CAMERA_WARMUP_FRAMES    frames discarded first, default: 20
  SP3_CAMERA_TIMEOUT          poll timeout seconds, default: 75

This is Android Emulator camera-source evidence, not physical-camera evidence.
EOF
}

[[ $# -ge 3 && $# -le 4 ]] || { usage; exit 2; }

SERIAL="$1"
PACKAGE="$2"
EXPECTED_HEX="$3"
ADB="${SP3_ADB_BIN:-adb}"
FRAME_COUNT="${SP3_CAMERA_FRAME_COUNT:-12}"
WARMUP_FRAMES="${SP3_CAMERA_WARMUP_FRAMES:-20}"
TIMEOUT_SECONDS="${SP3_CAMERA_TIMEOUT:-75}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
EVIDENCE="${4:-evidence/virtual-phone-avd-camera-optical-${STAMP}}"
mkdir -p "$EVIDENCE"

command -v "$ADB" >/dev/null
command -v cargo >/dev/null
command -v sha256sum >/dev/null

[[ "$EXPECTED_HEX" =~ ^[0-9a-fA-F]+$ ]] || {
  echo "expected_hex must be hexadecimal" >&2
  exit 2
}
(( ${#EXPECTED_HEX} % 2 == 0 )) || {
  echo "expected_hex must have an even number of digits" >&2
  exit 2
}
[[ "$FRAME_COUNT" =~ ^[0-9]+$ ]] || exit 2
[[ "$WARMUP_FRAMES" =~ ^[0-9]+$ ]] || exit 2
(( FRAME_COUNT >= 1 && FRAME_COUNT <= 120 )) || exit 2
(( WARMUP_FRAMES >= 0 && WARMUP_FRAMES <= 300 )) || exit 2

adb_run() {
  "$ADB" -s "$SERIAL" "$@"
}

state="$(adb_run get-state | tr -d '\r')"
[[ "$state" == "device" ]] || {
  echo "$SERIAL is not online" >&2
  exit 1
}

adb_run shell pm path "$PACKAGE" >/dev/null
adb_run shell pm grant "$PACKAGE" android.permission.CAMERA

if ! adb_run shell pm list features     | tr -d '\r'     | grep -q 'feature:android.hardware.camera'; then
  echo "$SERIAL lacks android.hardware.camera" >&2
  exit 1
fi

adb_run logcat -c

COMPONENT="$PACKAGE/.CameraOpticalCaptureActivity"
adb_run shell am start -W   -n "$COMPONENT"   --ei dev.nolane.sanpham3.recoverylab.CAMERA_FRAME_COUNT "$FRAME_COUNT"   --ei dev.nolane.sanpham3.recoverylab.CAMERA_WARMUP_FRAMES "$WARMUP_FRAMES"   --ei dev.nolane.sanpham3.recoverylab.CAMERA_WIDTH 640   --ei dev.nolane.sanpham3.recoverylab.CAMERA_HEIGHT 480   | tee "$EVIDENCE/am-start.txt"

PASS_LINE=""
for _ in $(seq 1 "$TIMEOUT_SECONDS"); do
  PASS_LINE="$(
    adb_run logcat -d -v brief -s SP3CameraOptical:I '*:S'       | tr -d '\r'       | grep 'CAMERA_VIDEO_SOURCE_PASS'       | tail -n 1       || true
  )"
  if [[ -n "$PASS_LINE" ]]; then
    break
  fi

  if adb_run logcat -d -v brief -s SP3CameraOptical:I '*:S'       | grep -q 'CAMERA_VIDEO_SOURCE_FAIL'; then
    break
  fi
  sleep 1
done

adb_run shell getprop >"$EVIDENCE/getprop.txt"
adb_run shell pm list features >"$EVIDENCE/features.txt"
adb_run shell dumpsys media.camera >"$EVIDENCE/media-camera.txt" || true
adb_run shell dumpsys package "$PACKAGE" >"$EVIDENCE/package.txt" || true
adb_run logcat -d -v threadtime -s SP3CameraOptical:I '*:S'   >"$EVIDENCE/camera-logcat.txt" || true

if [[ -z "$PASS_LINE" ]]; then
  echo "Camera optical capture did not PASS" >&2
  cat "$EVIDENCE/camera-logcat.txt" >&2 || true
  exit 1
fi

grep -q 'evidence_level=ANDROID_AVD_CAMERA' <<<"$PASS_LINE"
grep -q "frames=$FRAME_COUNT" <<<"$PASS_LINE"

adb_run exec-out run-as "$PACKAGE"   cat files/camera-optical/latest.y4m   >"$EVIDENCE/capture.y4m"

test -s "$EVIDENCE/capture.y4m"

cargo run -p signal-trace-replay-cli --   optical-y4m-auto-scale   "$EXPECTED_HEX"   "$EVIDENCE/capture.y4m"   0   "$FRAME_COUNT"   | tee "$EVIDENCE/replay.txt"

grep -q 'F3_OPTICAL_Y4M_REPLAY' "$EVIDENCE/replay.txt"
grep -q 'bit_errors=0' "$EVIDENCE/replay.txt"

{
  echo "timestamp_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "git_commit=$(git rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "serial=$SERIAL"
  echo "api=$(adb_run shell getprop ro.build.version.sdk | tr -d '\r')"
  echo "package=$PACKAGE"
  echo "expected_hex=$EXPECTED_HEX"
  echo "frame_count=$FRAME_COUNT"
  echo "warmup_frames=$WARMUP_FRAMES"
  echo "camera_pass=$PASS_LINE"
  echo "evidence_level=ANDROID_AVD_CAMERA"
} >"$EVIDENCE/metadata.txt"

(
  cd "$EVIDENCE"
  find . -maxdepth 1 -type f ! -name SHA256SUMS -print0     | sort -z     | xargs -0 sha256sum >SHA256SUMS
)

echo "F5_AVD_CAMERA_VIDEO_SOURCE_PASS serial=$SERIAL frames=$FRAME_COUNT capture=$EVIDENCE/capture.y4m"
